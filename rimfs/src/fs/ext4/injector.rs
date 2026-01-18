// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use core::convert::TryInto;

use crate::core::allocator::FsAllocator;
use crate::{
    core::{FsInjectorError, FsInjectorResult, injector::FsNodeInjector, traits::FileAttributes},
    fs::ext4::{
        allocator::{Ext4Allocator, Ext4Handle},
        constant::*,
        group_layout::GroupLayout,
        meta::Ext4Meta,
        types::{Ext4BgdtUpdate, Ext4DirEntry, Ext4Extent, Ext4Inode},
    },
};
use rimio::{RimIO, RimIOExt};
use zerocopy::IntoBytes;

use crate::fs::ext4::types::Ext4LostFound;

/// EXT4-specific directory context with child subdirectory tracking for link counts
struct Ext4Context {
    handle: Ext4Handle,
    buf: Vec<u8>,
    /// Number of immediate subdirectories (for parent link count calculation)
    child_dir_count: u16,
    /// Original extent for re-writing the inode
    extent: Ext4Extent,
}

impl Ext4Context {
    fn new(handle: Ext4Handle, buf: Vec<u8>, extent: Ext4Extent) -> Self {
        Self {
            handle,
            buf,
            child_dir_count: 0,
            extent,
        }
    }
}

pub struct Ext4Injector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: &'a mut Ext4Allocator<'a>,
    meta: &'a Ext4Meta,
    stack: Vec<Ext4Context>,
    /// Track used directories per group for BGDT
    used_dirs_per_group: Vec<u16>,
}

impl<'a, IO: RimIO + ?Sized> Ext4Injector<'a, IO> {
    pub fn new(io: &'a mut IO, allocator: &'a mut Ext4Allocator<'a>, params: &'a Ext4Meta) -> Self {
        let group_count = params.block_count.div_ceil(params.blocks_per_group) as usize;
        let mut used_dirs_per_group = vec![0u16; group_count];
        // Account for Root Directory (inode 2) and Lost+Found (inode 11) in Group 0
        if group_count > 0 {
            used_dirs_per_group[0] = 2;
        }

        Self {
            io,
            allocator,
            meta: params,
            stack: vec![],
            used_dirs_per_group,
        }
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.allocator.blocks.block_offset(block);
        self.io
            .write_block_best_effort(offset, data, self.meta.block_size as usize)?;
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
        let mut total_used_blocks = 0;
        let mut total_used_inodes = 0;

        for g in 0..self.group_count() {
            total_used_blocks += self.group_used_blocks(g);
            total_used_inodes += self.group_used_inodes(g);
        }

        let free_blocks = self.meta.block_count.saturating_sub(total_used_blocks);
        let free_inodes = self.meta.inode_count.saturating_sub(total_used_inodes);

        self.io
            .write_u32_at(EXT4_SUPERBLOCK_OFFSET + 0x0C, free_blocks)?; // s_free_blocks_count_lo
        self.io
            .write_u32_at(EXT4_SUPERBLOCK_OFFSET + 0x10, free_inodes)?; // s_free_inodes_count

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

    fn group_used_blocks(&self, group_index: usize) -> u32 {
        let layout = GroupLayout::compute(self.meta, group_index as u32);
        let metadata_overhead = layout.first_data_block - layout.group_start;
        let data_used = self.allocator.blocks.allocated_in_group(group_index);
        metadata_overhead + data_used
    }

    fn group_total_inodes(&self, group_index: usize) -> usize {
        if group_index < self.group_count() - 1 {
            self.meta.inodes_per_group as usize
        } else {
            (self.meta.inode_count as usize) - (group_index * self.meta.inodes_per_group as usize)
        }
    }

    fn group_used_inodes(&self, group_index: usize) -> u32 {
        self.allocator
            .meta
            .allocated_in_group(group_index, self.meta.inodes_per_group)
    }

    fn flush_bgdt(&mut self) -> FsInjectorResult {
        let mut offsets = Vec::with_capacity(self.group_count());
        let mut buffer = Vec::with_capacity(self.group_count() * 4);

        for group_index in 0..self.group_count() {
            let free_blocks =
                self.group_total_blocks(group_index) as u32 - self.group_used_blocks(group_index);
            let free_inodes =
                self.group_total_inodes(group_index) as u32 - self.group_used_inodes(group_index);
            let used_dirs = self
                .used_dirs_per_group
                .get(group_index)
                .copied()
                .unwrap_or(0);

            let update = Ext4BgdtUpdate::new(free_blocks as u16, free_inodes as u16, used_dirs);

            let offset = self.bgdt_offset() + (group_index as u64) * EXT4_BGDT_ENTRY_SIZE as u64;

            offsets.push(offset + 0x0C);
            buffer.extend_from_slice(update.as_bytes());
        }

        self.io.write_multi_at(&offsets, 6, &buffer)?;

        Ok(())
    }

    /// Pad a directory block buffer so that the last entry's rec_len extends to block end.
    /// This is required by e2fsck - the last entry must span to the end of the block.
    fn pad_directory_block(&self, buf: &mut Vec<u8>) {
        let block_size = self.meta.block_size as usize;

        if buf.is_empty() || buf.len() >= block_size {
            buf.resize(block_size, 0);
            return;
        }

        // Find the last entry and adjust its rec_len
        let mut pos = 0usize;
        let mut last_entry_pos = 0usize;

        while pos + 8 <= buf.len() {
            let rec_len = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
            if rec_len == 0 || pos + rec_len > buf.len() {
                break;
            }
            last_entry_pos = pos;
            pos += rec_len;
        }

        // Adjust the last entry's rec_len to reach block_size
        let new_rec_len = (block_size - last_entry_pos) as u16;
        buf[last_entry_pos + 4] = (new_rec_len & 0xFF) as u8;
        buf[last_entry_pos + 5] = ((new_rec_len >> 8) & 0xFF) as u8;

        // Pad buffer to block size
        buf.resize(block_size, 0);
    }

    /// Flush block and inode bitmaps to reflect allocated state
    fn flush_bitmaps(&mut self) -> FsInjectorResult {
        use crate::core::utils::bitmap::BitmapOps;

        for group in 0..self.group_count() {
            let layout = GroupLayout::compute(self.meta, group as u32);

            // Block bitmap
            let bitmap_size = (self.meta.blocks_per_group / 8) as usize;
            let mut block_bitmap = vec![0xFFu8; bitmap_size]; // Start with all 1s (padding)

            // Calculate actual blocks in this group
            let group_block_count = self.group_total_blocks(group) as u32;

            // Clear valid block bits first (0..count)
            for i in 0..group_block_count {
                block_bitmap.set_bit(i as usize, false);
            }

            // Mark reserved blocks (Superblock, GDT, Bitmaps, Inode Table)
            // They are all before first_data_block
            let reserved_count = layout.first_data_block - layout.group_start;
            for i in 0..reserved_count {
                block_bitmap.set_bit(i as usize, true);
            }

            // Mark allocated data blocks
            let allocated_data = self.allocator.blocks.allocated_in_group(group);
            let first_data_offset = layout.first_data_block - layout.group_start;
            for i in 0..allocated_data {
                let bit = first_data_offset + i;
                block_bitmap.set_bit(bit as usize, true);
            }

            // Pad bitmap to full block size with 1s (0xFF)
            // e2fsck expects padding bits/bytes to be "set" (1).
            if block_bitmap.len() < self.meta.block_size as usize {
                block_bitmap.resize(self.meta.block_size as usize, 0xFF);
            }

            // Write block bitmap
            let block_offset = layout.block_bitmap_block as u64 * self.meta.block_size as u64;
            self.io.write_block_best_effort(
                block_offset,
                &block_bitmap,
                self.meta.block_size as usize,
            )?;

            // Inode bitmap
            let inode_bitmap_size = (self.meta.inodes_per_group / 8) as usize;
            let mut inode_bitmap = vec![0xFFu8; inode_bitmap_size];

            // Calculate actual inodes
            let group_inode_count = self.group_total_inodes(group) as u32;

            // Clear valid inode bits
            for i in 0..group_inode_count {
                inode_bitmap.set_bit(i as usize, false);
            }

            // Mark used inodes
            // allocated_in_group returns count of inodes used in this group (1-based index)
            // The inodes in this group are [group_start_inode + 1 .. group_start_inode + inodes_per_group]
            // We need to mark bits 0 .. allocated-1.
            let allocated_inodes = self
                .allocator
                .meta
                .allocated_in_group(group, self.meta.inodes_per_group);
            for i in 0..allocated_inodes {
                inode_bitmap.set_bit(i as usize, true);
            }

            let inode_offset = layout.inode_bitmap_block as u64 * self.meta.block_size as u64;

            // Pad inode bitmap to full block size with 1s (0xFF)
            if inode_bitmap.len() < self.meta.block_size as usize {
                inode_bitmap.resize(self.meta.block_size as usize, 0xFF);
            }

            self.io.write_block_best_effort(
                inode_offset,
                &inode_bitmap,
                self.meta.block_size as usize,
            )?;
        }

        Ok(())
    }

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        self.flush_bitmaps()?;
        self.flush_superblock()?;
        self.flush_bgdt()?;
        Ok(())
    }

    fn create_lost_found(&mut self) -> FsInjectorResult {
        // If we are here, lost+found is MISSING from Root.
        // We need to:
        // 1. Allocate a block for it
        // 2. Write the directory content (using Ext4LostFound)
        // 3. Write the Inode (using Ext4LostFound, forced to inode 11)
        // 4. Register it in the parent (Root) directory buffer

        // 1. Allocate a block
        // We use standard allocation. It might not be contiguous to root, but that's fine.
        let handle = self
            .allocator
            .allocate_chain(1)
            .map_err(|_| FsInjectorError::Other("Allocation failed for lost+found"))?;
        let block = handle.blocks[0];

        // 2. Write directory content
        let dir_buf = Ext4LostFound::create_dir_block(self.meta.block_size as usize);
        self.write_block(block, &dir_buf)?;

        // 3. Write Inode (Inode 11)
        let inode = Ext4LostFound::INODE;
        let inode_data = Ext4LostFound::create_inode(self.meta.block_size, block);
        let inode_buf = inode_data.to_bytes();
        self.write_metadata(inode, &inode_buf)?;

        // 4. Add entry to parent (Root) buffer
        // Note: `self.stack.last_mut()` is Root context at this point
        if let Some(parent) = self.stack.last_mut() {
            let entry = Ext4LostFound::entry();
            entry.to_raw_buffer(&mut parent.buf);

            // Increment parent link count (subdirectories increase parent nlink)
            parent.child_dir_count += 1;

            // Mark directory as used in group stats
            let inode_index = inode - 1;
            let group = (inode_index / self.meta.inodes_per_group) as usize;
            if let Some(count) = self.used_dirs_per_group.get_mut(group) {
                *count += 1;
            }
        }

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

        // Keep only the existing entries (. and .. and lost+found)
        existing.truncate(entries_end);

        // Count existing subdirectories for link count
        let mut child_dir_count = 0u16;
        let mut pos = 0usize;
        while pos + 8 <= existing.len() {
            let rec_len = u16::from_le_bytes([existing[pos + 4], existing[pos + 5]]) as usize;
            if rec_len == 0 {
                break;
            }

            let file_type = existing[pos + 7];
            let name_len = existing[pos + 6] as usize;
            let name = &existing[pos + 8..pos + 8 + name_len];

            // Count directories, but ignore "." and ".."
            if file_type == EXT4_FT_DIR && name != b"." && name != b".." {
                child_dir_count += 1;
            }
            pos += rec_len;
        }

        // Create handle for root (using existing block, inode 2)
        let handle = Ext4Handle::new(root_inode, vec![root_block]);
        let extent = Ext4Extent::new(0, root_block, 1);

        let mut ctx = Ext4Context::new(handle, existing, extent);
        ctx.child_dir_count = child_dir_count;
        self.stack.push(ctx);

        // Check if `lost+found` exists
        let mut has_lost_found = false;
        if let Some(ctx) = self.stack.last() {
            let mut pos = 0usize;
            while pos + 8 <= ctx.buf.len() {
                let rec_len = u16::from_le_bytes([ctx.buf[pos + 4], ctx.buf[pos + 5]]) as usize;
                if rec_len == 0 {
                    break;
                }
                let name_len = ctx.buf[pos + 6] as usize;
                let name_start = pos + 8;
                if name_start + name_len <= ctx.buf.len() {
                    let name = &ctx.buf[name_start..name_start + name_len];
                    if name == b"lost+found" {
                        has_lost_found = true;
                        break;
                    }
                }
                pos += rec_len;
            }
        }

        if !has_lost_found {
            self.create_lost_found()?;
        }

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
        Ext4DirEntry::dot(inode).to_raw_buffer(&mut entries);

        // Parent inode
        let parent_inode = self
            .stack
            .last()
            .map(|c| c.handle.inode)
            .unwrap_or(EXT4_ROOT_INODE);
        Ext4DirEntry::dotdot(parent_inode).to_raw_buffer(&mut entries);

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
            entry.to_raw_buffer(&mut parent.buf);
        }

        // Push new dir context
        let ctx = Ext4Context::new(handle, entries, extent);
        self.stack.push(ctx);

        // Track used dir count
        let inode_index = inode - 1;
        let group = (inode_index / self.meta.inodes_per_group) as usize;
        if let Some(count) = self.used_dirs_per_group.get_mut(group) {
            *count += 1;
        }

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
            entry.to_raw_buffer(&mut ctx.buf);
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            // Pad directory block so last entry spans to end
            self.pad_directory_block(&mut ctx.buf);
            // Write to first block. Logic limitation: directory size <= 1 block
            if let Some(&block_id) = ctx.handle.blocks.first() {
                self.write_block(block_id, &ctx.buf)?;
            }

            // Re-write this directory's inode with correct link count
            // Link count = 2 (for . and ..) + child_dir_count (subdirs pointing back via ..)
            let links = 2 + ctx.child_dir_count;
            let inode_data = Ext4Inode::from_attr(
                &FileAttributes::new_dir(),
                self.meta.block_size as u64,
                links,
                self.meta.block_size.div_ceil(512),
                &[ctx.extent],
            );
            self.write_metadata(ctx.handle.inode, &inode_data.to_bytes())?;

            // Increment parent's child_dir_count (this dir is a subdirectory of parent)
            if let Some(parent) = self.stack.last_mut() {
                parent.child_dir_count += 1;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        // Drain the stack by calling flush_current repeatedly
        // This ensures each directory gets its link count updated correctly
        while !self.stack.is_empty() {
            self.flush_current()?;
        }

        self.flush_metadata()?;
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::core::traits::FsResolver;
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

        // Parse tree back (skipping lost+found to avoid recursion/overflow issues in test)
        // We manually traverse the root to filter out lost+found before building the tree node
        let mut parser_back = Ext4Resolver::new(&mut io, &meta);
        let root_children = parser_back.read_dir("/").expect("read_dir / failed");

        let mut children = vec![];
        for name in root_children {
            if name == "lost+found" {
                continue;
            }
            let path = format!("/{name}");
            let child = parser_back
                .build_node(&path, true)
                .expect("build_node failed");
            children.push(child);
        }
        // Restore children sort order for structural comparison
        children.sort_by_key(|c| c.name().to_ascii_lowercase());

        let mut parsed_tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children,
        };

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
