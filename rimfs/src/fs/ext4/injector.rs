// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::{
    fs::ext4::{constant::*, encoder::Ext4Encoder, params::Ext4Params},
    core::{
        FsInjectorError,
        allocator::{FsAllocator, FsMetadataAllocator},
        error::FsInjectorResult,
        injector::*,
        io::FsBlockIO,
        parser::attr::FileAttributes,
    },
};

pub struct Ext4Injector<
    'a,
    IO: FsBlockIO<u32>,
    BA: FsAllocator<u32>,
    MA: FsMetadataAllocator<u32>,
> {
    io: &'a mut IO,
    block_allocator: &'a mut BA,
    metadata_allocator: &'a mut MA,
    params: &'a Ext4Params,
    stack: Vec<FsNodeContext<u32>>,
}

impl<'a, IO: FsBlockIO<u32>, BA: FsAllocator<u32>, MA: FsMetadataAllocator<u32>>
    Ext4Injector<'a, IO, BA, MA>
{
    pub fn new(
        io: &'a mut IO,
        block_allocator: &'a mut BA,
        metadata_allocator: &'a mut MA,
        params: &'a Ext4Params,
    ) -> Self {
        Self {
            io,
            block_allocator,
            metadata_allocator,
            params,
            stack: vec![],
        }
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.block_allocator.block_offset(block);
        self.io.write_at(offset, data)?;
        Ok(())
    }

    fn write_metadata(&mut self, metadata_id: u32, data: &[u8]) -> FsInjectorResult {
        if let Some(offset) = self.block_allocator.metadata_offset(metadata_id) {
            self.io.write_at(offset, data)?;
            Ok(())
        } else {
            Err(FsInjectorError::InvalidMetadataId(metadata_id))
        }
    }

    fn flush_superblock(&mut self) -> FsInjectorResult {
        let free_blocks =
            self.block_allocator.total_blocks_count() - self.block_allocator.used_blocks();
        let free_inodes = self.metadata_allocator.total_metadata_count()
            - self.metadata_allocator.used_metadata();

        let mut sb_update = [0u8; 8];

        sb_update[0..4].copy_from_slice(&(free_blocks as u32).to_le_bytes()); // s_free_blocks_count_lo
        sb_update[4..8].copy_from_slice(&(free_inodes as u32).to_le_bytes()); // s_free_inodes_count

        self.io
            .write_at(EXT4_SUPERBLOCK_OFFSET + 0x0C, &sb_update)?;

        Ok(())
    }

    fn group_count(&self) -> usize {
        ((self.params.block_count + self.params.blocks_per_group - 1)
            / self.params.blocks_per_group) as usize
    }

    fn bgdt_offset(&self) -> u64 {
        let first_data_block = self.params.first_data_block as u64;
        (first_data_block + 1) * self.params.block_size as u64
    }

    fn group_total_blocks(&self, _group_index: usize) -> usize {
        // Tous les groupes sauf le dernier sont "pleins"
        if _group_index < self.group_count() - 1 {
            self.params.blocks_per_group as usize
        } else {
            // Le dernier groupe peut être "partiel"
            (self.params.block_count as usize)
                - (_group_index * self.params.blocks_per_group as usize)
        }
    }

    fn group_used_blocks(&self, group_index: usize) -> usize {
        let current_global_block = self.block_allocator.used_blocks();

        let group_start_block = group_index * self.params.blocks_per_group as usize;
        let group_end_block = group_start_block + self.group_total_blocks(group_index);

        if current_global_block <= group_start_block {
            0
        } else if current_global_block >= group_end_block {
            self.group_total_blocks(group_index)
        } else {
            current_global_block - group_start_block
        }
    }

    fn group_total_inodes(&self, group_index: usize) -> usize {
        if group_index < self.group_count() - 1 {
            self.params.inodes_per_group as usize
        } else {
            (self.params.inode_count as usize)
                - (group_index * self.params.inodes_per_group as usize)
        }
    }

    fn group_used_inodes(&self, group_index: usize) -> usize {
        let current_used = self.metadata_allocator.used_metadata();

        let group_start_inode = group_index * self.params.inodes_per_group as usize;
        let group_end_inode = group_start_inode + self.group_total_inodes(group_index);

        if current_used <= group_start_inode {
            0
        } else if current_used >= group_end_inode {
            self.group_total_inodes(group_index)
        } else {
            current_used - group_start_inode
        }
    }

    fn flush_bgdt(&mut self) -> FsInjectorResult {
        for group_index in 0..self.group_count() {
            let free_blocks =
                self.group_total_blocks(group_index) - self.group_used_blocks(group_index);
            let free_inodes =
                self.group_total_inodes(group_index) - self.group_used_inodes(group_index);

            let mut bg_update = [0u8; 4];

            bg_update[0..2].copy_from_slice(&(free_blocks as u16).to_le_bytes()); // bg_free_blocks_count
            bg_update[2..4].copy_from_slice(&(free_inodes as u16).to_le_bytes()); // bg_free_inodes_count

            let offset = self.bgdt_offset() + (group_index as u64) * EXT4_BGDT_ENTRY_SIZE as u64;

            self.io.write_at(offset + 0x0C, &bg_update)?;
        }

        Ok(())
    }

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        self.flush_superblock()?;
        self.flush_bgdt()?;
        println!("[ext4] Metadata flushed: superblock + BGDT updated.");
        Ok(())
    }
}

impl<'a, IO: FsBlockIO<u32>, BA: FsAllocator<u32>, MA: FsMetadataAllocator<u32>>
    FsInjector<u32> for Ext4Injector<'a, IO, BA, MA>
{
    fn begin(&mut self) -> FsInjectorResult {
        // Root context
        let root_block = self.block_allocator.allocate_block();
        let root_inode = self.metadata_allocator.allocate_metadata_id();

        let root_inode_buf = Ext4Encoder::encode_inode_from_attr(
            &FileAttributes::new_folder(),
            self.block_allocator.block_size() as u32,
            EXT4_ROOT_DIR_LINKS_COUNT,
            (self.block_allocator.block_size() as u32).div_ceil(512),
            root_block,
        );
        self.write_metadata(root_inode, &root_inode_buf)?;

        let ctx = FsNodeContext::new("/".to_string(), Some(root_block), None, {
            let mut e = vec![];
            e.extend_from_slice(&Ext4Encoder::dot_entry(EXT4_ROOT_INODE));
            e.extend_from_slice(&Ext4Encoder::dotdot_entry(EXT4_ROOT_INODE));
            e
        });

        self.stack.push(ctx);
        Ok(())
    }

    fn dir_with_attr(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        // Allocate inode and block for new dir
        let inode = self.metadata_allocator.allocate_metadata_id();
        let block = self.block_allocator.allocate_block();

        // Write "." and ".."
        let mut entries = vec![];
        entries.extend_from_slice(&Ext4Encoder::dot_entry(inode));
        entries.extend_from_slice(&Ext4Encoder::dotdot_entry(
            self.stack.last().unwrap().first_block.unwrap_or(2),
        ));

        let inode_buf = Ext4Encoder::encode_inode_from_attr(
            attr,
            self.params.block_size,
            if attr.dir { 2 } else { 1 }, // dir = 2 links (., ..), file = 1
            self.params.block_size.div_ceil(512),
            block,
        );

        self.write_metadata(inode, &inode_buf)?;

        // Add entry to parent dir
        let entry = Ext4Encoder::dir_entry_from_attr(name, attr, inode);
        if let Some(parent) = self.stack.last_mut() {
            parent.entries.extend_from_slice(&entry);
        }

        // Push new dir context
        let ctx = FsNodeContext::new(name.to_string(), Some(block), Some(inode), entries);
        self.stack.push(ctx);

        Ok(())
    }

    fn dir(&mut self, name: &str) -> FsInjectorResult {
        self.dir_with_attr(name, &FileAttributes::new_folder())
    }

    fn file_with_attr(
        &mut self,
        name: &str,
        content: &[u8],
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        // Allocate inode and block
        let inode = self.metadata_allocator.allocate_metadata_id();
        let block = self.block_allocator.allocate_block();

        // Write content
        self.write_block(block, content)?;

        let inode_buf = Ext4Encoder::encode_inode_from_attr(
            attr,
            content.len() as u32,
            if attr.dir { 2 } else { 1 }, // dir = 2 links (., ..), file = 1
            (content.len() as u32).div_ceil(512),
            block,
        );

        self.write_metadata(inode, &inode_buf)?;

        // Add entry to current dir
        let entry = Ext4Encoder::file_entry_from_attr(name, attr, inode);
        if let Some(ctx) = self.stack.last_mut() {
            ctx.entries.extend_from_slice(&entry);
        }

        Ok(())
    }

    fn file(&mut self, name: &str, content: &[u8]) -> FsInjectorResult {
        self.file_with_attr(name, content, &FileAttributes::new_file())
    }

    fn end(&mut self) -> FsInjectorResult {
        let ctx = self.stack.pop().expect("end called on empty stack!");
        let block_id = ctx.first_block.expect("no first block set!");

        // Write entries as dir data
        self.write_block(block_id, &ctx.entries)?;

        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        assert_eq!(self.stack.len(), 1, "flush called but stack not clean!");
        let ctx = self.stack.pop().unwrap();
        let block_id = ctx.first_block.expect("root first block missing!");

        // Write root entries
        self.write_block(block_id, &ctx.entries)?;

        self.flush_metadata()?;

        self.io.flush()?;

        println!(
            "[ext4] Injector summary: used_inodes = {}",
            self.metadata_allocator.used_metadata()
        );

        Ok(())
    }
}
