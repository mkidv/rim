// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use core::convert::TryInto;

use crate::core::allocator::FsAllocator;
use crate::{
    core::{
        FsInjectorError, FsInjectorResult,
        injector::{FsContext, FsNodeInjector},
        traits::FileAttributes,
    },
    fs::ext4::{
        allocator::{Ext4Allocator, Ext4Handle},
        constant::*,
        group_layout::GroupLayout,
        meta::Ext4Meta,
        types::{Ext4DirEntry, Ext4Extent, Ext4Inode},
    },
};
use rimio::{RimIO, RimIOExt};

pub struct Ext4Injector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: &'a mut Ext4Allocator<'a>,
    meta: &'a Ext4Meta,
    stack: Vec<FsContext<Ext4Handle>>,
}

impl<'a, IO: RimIO + ?Sized> Ext4Injector<'a, IO> {
    pub fn new(io: &'a mut IO, allocator: &'a mut Ext4Allocator<'a>, params: &'a Ext4Meta) -> Self {
        Self {
            io,
            allocator,
            meta: params,
            stack: vec![],
        }
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.allocator.blocks.block_offset(block);
        self.io.write_at(offset, data)?;
        Ok(())
    }

    fn write_metadata(&mut self, metadata_id: u32, data: &[u8]) -> FsInjectorResult {
        // inode numbers are 1-based.
        if metadata_id < 1 {
            return Err(FsInjectorError::Other("Invalid Inode 0"));
        }
        let inode_index = metadata_id - 1;
        let inodes_per_group = self.meta.inodes_per_group;
        let group = inode_index / inodes_per_group;
        let index_in_group = inode_index % inodes_per_group;

        let layout = GroupLayout::compute(self.meta, group);
        let table_block = layout.inode_table_block;

        // inode size? EXT4_DEFAULT_INODE_SIZE
        let inode_size = EXT4_DEFAULT_INODE_SIZE as u64;
        let offset = (table_block as u64 * self.meta.block_size as u64)
            + (index_in_group as u64 * inode_size);

        self.io.write_at(offset, data)?;
        Ok(())
    }

    fn flush_superblock(&mut self) -> FsInjectorResult {
        let free_blocks = self.meta.block_count - self.allocator.blocks.used_units() as u32;
        let free_inodes = self.allocator.meta.total_metadata_count() as u32
            - self.allocator.meta.used_metadata() as u32;

        let mut sb_update = [0u8; 8];

        sb_update[0..4].copy_from_slice(&free_blocks.to_le_bytes()); // s_free_blocks_count_lo
        sb_update[4..8].copy_from_slice(&(free_inodes).to_le_bytes()); // s_free_inodes_count

        self.io
            .write_at(EXT4_SUPERBLOCK_OFFSET + 0x0C, &sb_update)?;

        Ok(())
    }

    fn group_count(&self) -> usize {
        self.meta.block_count.div_ceil(self.meta.blocks_per_group) as usize
    }

    fn bgdt_offset(&self) -> u64 {
        let first_data_block = self.meta.first_data_block as u64;
        (first_data_block + 1) * self.meta.block_size as u64
    }

    fn group_total_blocks(&self, _group_index: usize) -> usize {
        if _group_index < self.group_count() - 1 {
            self.meta.blocks_per_group as usize
        } else {
            (self.meta.block_count as usize) - (_group_index * self.meta.blocks_per_group as usize)
        }
    }

    fn group_used_blocks(&self, group_index: usize) -> usize {
        let current_global_block = self.allocator.blocks.used_units();

        let group_start_block = group_index * self.meta.blocks_per_group as usize;
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
            self.meta.inodes_per_group as usize
        } else {
            (self.meta.inode_count as usize) - (group_index * self.meta.inodes_per_group as usize)
        }
    }

    fn group_used_inodes(&self, group_index: usize) -> usize {
        let current_used = self.allocator.meta.used_metadata();

        let group_start_inode = group_index * self.meta.inodes_per_group as usize;
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
        let mut offsets = Vec::with_capacity(self.group_count());
        let mut buffer = Vec::with_capacity(self.group_count() * 4);

        for group_index in 0..self.group_count() {
            let free_blocks =
                self.group_total_blocks(group_index) - self.group_used_blocks(group_index);
            let free_inodes =
                self.group_total_inodes(group_index) - self.group_used_inodes(group_index);

            let mut bg_update = [0u8; 4];
            bg_update[0..2].copy_from_slice(&(free_blocks as u16).to_le_bytes()); // bg_free_blocks_count
            bg_update[2..4].copy_from_slice(&(free_inodes as u16).to_le_bytes()); // bg_free_inodes_count

            let offset = self.bgdt_offset() + (group_index as u64) * EXT4_BGDT_ENTRY_SIZE as u64;

            offsets.push(offset + 0x0C);
            buffer.extend_from_slice(&bg_update);
        }

        self.io.write_multi_at(&offsets, 4, &buffer)?;

        Ok(())
    }

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        self.flush_superblock()?;
        self.flush_bgdt()?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsNodeInjector<Ext4Handle> for Ext4Injector<'a, IO> {
    fn set_root_context(&mut self, _root: &crate::core::traits::FsNode) -> FsInjectorResult {
        // Use the pre-formatted root inode (inode 2), not allocating a new one.
        // The root directory was already written by the formatter.

        // First, get the root inode info to find its data block
        let root_inode = EXT4_ROOT_INODE;

        // Read existing root directory block (the one written by formatter)
        let layout = GroupLayout::compute(self.meta, 0);
        let root_block = layout.first_data_block;

        // Read existing directory content
        let mut existing = vec![0u8; self.meta.block_size as usize];
        let offset = root_block as u64 * self.meta.block_size as u64;
        self.io.read_at(offset, &mut existing)?;

        // Find end of existing entries (look for first rec_len that would exceed block size or inode=0)
        let mut pos = 0usize;
        let mut entries_end = 0usize;
        while pos + 8 <= existing.len() {
            let entry_inode_bytes: [u8; 4] = existing[pos..pos + 4].try_into().unwrap_or([0; 4]);
            let entry_inode = u32::from_le_bytes(entry_inode_bytes);

            let rec_len_bytes: [u8; 2] = existing[pos + 4..pos + 6].try_into().unwrap_or([0; 2]);
            let rec_len = u16::from_le_bytes(rec_len_bytes) as usize;

            if entry_inode == 0 || rec_len == 0 || rec_len > existing.len() - pos {
                entries_end = pos;
                break;
            }
            pos += rec_len;
            entries_end = pos;
        }

        // Keep only the existing entries (. and ..)
        existing.truncate(entries_end);

        // Create handle for root (using existing block, inode 2)
        let handle = Ext4Handle::new(root_inode, vec![root_block]);

        let ctx = FsContext::new(handle, existing);
        self.stack.push(ctx);
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        // Allocate inode and block for new dir
        let handle = self
            .allocator
            .allocate_chain(1)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let block = handle.blocks[0];

        // Write "." and ".."
        let mut entries = vec![];
        entries.extend_from_slice(&Ext4DirEntry::dot(inode).to_bytes());

        // Parent inode
        let parent_inode = self
            .stack
            .last()
            .map(|c| c.handle.inode)
            .unwrap_or(EXT4_ROOT_INODE);
        entries.extend_from_slice(&Ext4DirEntry::dotdot(parent_inode).to_bytes());

        let extent = Ext4Extent::new(0, block, 1);
        let inode_data = Ext4Inode::from_attr(
            attr,
            self.meta.block_size as u64,
            if attr.dir { 2 } else { 1 },
            self.meta.block_size.div_ceil(512),
            &[extent],
        );
        let inode_buf = inode_data.to_bytes();

        self.write_metadata(inode, &inode_buf)?;

        // Add entry to parent dir
        let entry = Ext4DirEntry::from_attr(inode, name, attr);
        if let Some(parent) = self.stack.last_mut() {
            parent.buf.extend_from_slice(&entry.to_bytes());
        }

        // Push new dir context
        let ctx = FsContext::new(handle, entries);
        self.stack.push(ctx);

        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimIO,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        // Allocate inode and blocks
        let total_size = size as u32;
        let block_size = self.meta.block_size;
        let blocks_needed = total_size.div_ceil(block_size) as usize;

        let handle = self
            .allocator
            .allocate_chain(blocks_needed)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let blocks = handle.blocks;

        // Write content using streaming
        // We iterate over allocated blocks and copy data chunk by chunk.

        use crate::core::utils::stream_copy::write_stream_to_units;

        // ... (in write_file) ...
        // Stream content to disk
        if !blocks.is_empty() {
            write_stream_to_units(self.io, self.meta, source, &blocks, size)?;
        }

        // Build extents
        use crate::fs::ext4::types::Ext4Extent;
        let mut extents = Vec::new();
        if !blocks.is_empty() {
            let mut current_start = blocks[0];
            let mut current_len = 0;
            let mut logical_offset = 0;

            for &blk in &blocks {
                if blk == current_start + current_len {
                    current_len += 1;
                } else {
                    extents.push(Ext4Extent::new(
                        logical_offset,
                        current_start,
                        current_len as u16,
                    ));
                    logical_offset += current_len;
                    current_start = blk;
                    current_len = 1;
                }
            }
            extents.push(Ext4Extent::new(
                logical_offset,
                current_start,
                current_len as u16,
            ));
        }

        let inode_data = Ext4Inode::from_attr(
            attr,
            total_size as u64,
            if attr.dir { 2 } else { 1 },
            (blocks.len() as u32) * (block_size.div_ceil(512)),
            &extents,
        );
        let inode_buf = inode_data.to_bytes();

        self.write_metadata(inode, &inode_buf)?;

        // Add entry to current dir
        let entry = Ext4DirEntry::from_attr(inode, name, attr);
        if let Some(ctx) = self.stack.last_mut() {
            ctx.buf.extend_from_slice(&entry.to_bytes());
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(ctx) = self.stack.pop() {
            // Write to first block. Logic limitation: directory size <= 1 block
            if let Some(&block_id) = ctx.handle.blocks.first() {
                self.write_block(block_id, &ctx.buf)?;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        while let Some(ctx) = self.stack.pop() {
            if let Some(&block_id) = ctx.handle.blocks.first() {
                self.write_block(block_id, &ctx.buf)?;
            }
        }

        self.flush_metadata()?;
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::fs::ext4::prelude::*;

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    #[test]
    fn test_ext4_injector_hierarchy_flow() {
        let meta = Ext4Meta::new(SIZE_BYTES, Some("TESTFS"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        // Format the filesystem first
        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut allocator = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut allocator, &meta);

        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Dir {
                    name: "subdir".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::File {
                        name: "hello.txt".to_string(),
                        content: b"Hello World!".to_vec(),
                        attr: FileAttributes::new_file(),
                    }],
                },
                FsNode::File {
                    name: "readme.md".to_string(),
                    content: b"Test Readme".to_vec(),
                    attr: FileAttributes::new_file(),
                },
            ],
        };

        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Verify with checker
        let mut checker = Ext4Checker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        // Parse tree back
        let mut parser_back = Ext4Resolver::new(&mut io, &meta);
        let mut parsed_tree = parser_back.parse_tree("/*").expect("parse_tree failed");

        // Sort for comparison
        let mut tree_sorted = tree.clone();
        tree_sorted.sort_children_recursively();
        parsed_tree.sort_children_recursively();

        println!("{tree_sorted}");
        println!("{parsed_tree}");

        // Use structural_eq to ignore timestamps/mode differences
        assert!(
            tree_sorted.structural_eq(&parsed_tree),
            "Tree structure mismatch"
        );
    }

    #[test]
    fn test_ext4_injector_single_file() {
        let meta = Ext4Meta::new(SIZE_BYTES, Some("SINGLE"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut allocator = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut allocator, &meta);

        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::File {
                name: "test.txt".to_string(),
                content: b"Test content".to_vec(),
                attr: FileAttributes::new_file(),
            }],
        };

        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Verify with checker
        let mut checker = Ext4Checker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        // Read the file back
        let mut resolver = Ext4Resolver::new(&mut io, &meta);
        let content = resolver.read_file("/test.txt").expect("read_file failed");
        assert_eq!(content, b"Test content");

        println!("✓ Single file injection verified");
    }

    #[test]
    fn test_ext4_injector_nested_dirs() {
        let meta = Ext4Meta::new(SIZE_BYTES, Some("NESTED"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut allocator = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut allocator, &meta);

        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::Dir {
                name: "level1".to_string(),
                attr: FileAttributes::new_dir(),
                children: vec![FsNode::Dir {
                    name: "level2".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::File {
                        name: "deep.txt".to_string(),
                        content: b"Deep file".to_vec(),
                        attr: FileAttributes::new_file(),
                    }],
                }],
            }],
        };

        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Read nested file
        let mut resolver = Ext4Resolver::new(&mut io, &meta);
        let content = resolver
            .read_file("/level1/level2/deep.txt")
            .expect("read_file failed");
        assert_eq!(content, b"Deep file");

        // Verify directories exist
        let l1_entries = resolver.read_dir("/level1").expect("read_dir failed");
        assert!(
            l1_entries.contains(&"level2".to_string()),
            "level2 not found in level1"
        );

        println!("✓ Nested directories injection verified");
    }

    #[test]
    fn test_ext4_injector_large_file() {
        let meta = Ext4Meta::new(SIZE_BYTES, Some("LARGE"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut allocator = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut allocator, &meta);

        // Create a file larger than one block
        let large_content = vec![0xABu8; (meta.block_size as usize) * 3 + 512];

        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::File {
                name: "large.bin".to_string(),
                content: large_content.clone(),
                attr: FileAttributes::new_file(),
            }],
        };

        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Read the file back
        let mut resolver = Ext4Resolver::new(&mut io, &meta);
        let content = resolver.read_file("/large.bin").expect("read_file failed");
        assert_eq!(
            content.len(),
            large_content.len(),
            "Large file size mismatch"
        );
        assert_eq!(content, large_content, "Large file content mismatch");

        println!(
            "✓ Large file injection verified (size: {} bytes)",
            large_content.len()
        );
    }

    #[test]
    fn test_ext4_injector_empty_file() {
        let meta = Ext4Meta::new(SIZE_BYTES, Some("EMPTY"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut allocator = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut allocator, &meta);

        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::File {
                name: "empty.txt".to_string(),
                content: vec![],
                attr: FileAttributes::new_file(),
            }],
        };

        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Read the file back
        let mut resolver = Ext4Resolver::new(&mut io, &meta);
        let content = resolver.read_file("/empty.txt").expect("read_file failed");
        assert!(content.is_empty(), "Empty file should have no content");

        println!("✓ Empty file injection verified");
    }
}
