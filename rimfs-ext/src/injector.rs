// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use core::convert::TryInto;

use crate::core::allocator::FsAllocator;
use crate::{
    core::{FsInjectorError, FsInjectorResult, injector::*, traits::FileAttributes},
    {
        allocator::{ExtAllocator, ExtHandle},
        constant::*,
        group_layout::GroupLayout,
        meta::ExtMeta,
        ops,
        types::{ExtDirEntry, ExtExtent, ExtInode},
        updates,
    },
};
use rimio::prelude::*;
use rimio::{RimIO, RimIOExt};

/// EXT-specific directory context with child subdirectory tracking for link counts
struct ExtContext {
    handle: ExtHandle,
    buf: Vec<u8>,
    /// Number of immediate subdirectories (for parent link count calculation)
    child_dir_count: u16,
    /// Original extent for re-writing the inode
    extent: ExtExtent,
}

impl ExtContext {
    fn new(handle: ExtHandle, buf: Vec<u8>, extent: ExtExtent) -> Self {
        Self {
            handle,
            buf,
            child_dir_count: 0,
            extent,
        }
    }
}

pub struct ExtInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: ExtAllocator<'a>,
    meta: &'a ExtMeta,
    stack: Vec<ExtContext>,
    /// Track used directories per group for BGDT
    used_dirs_per_group: Vec<u16>,
}

impl<'a, IO: RimIO + ?Sized> ExtInjector<'a, IO> {
    pub fn new(io: &'a mut IO, params: &'a ExtMeta) -> Self {
        let allocator = ExtAllocator::new(params);
        let group_count = params.block_count.div_ceil(params.blocks_per_group as u64) as usize;
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

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        updates::flush_superblock(self.io, &self.allocator, self.meta)?;
        updates::flush_bgdt(
            self.io,
            &self.allocator,
            self.meta,
            &self.used_dirs_per_group,
        )?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<ExtHandle> for ExtInjector<'a, IO> {
    fn set_root_context(&mut self, _root: &crate::core::traits::FsNode) -> FsInjectorResult {
        // Use the pre-formatted root inode (inode 2), not allocating a new one.
        // The root directory was already written by the formatter.

        // First, get the root inode info to find its data block
        let root_inode = EXT_ROOT_INODE;

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
            if file_type == EXT_FT_DIR && name != b"." && name != b".." {
                child_dir_count += 1;
            }
            pos += rec_len;
        }

        // Create handle for root (using existing block, inode 2)
        let mut root_blocks = RunList::new();
        root_blocks.push(Run::new(root_block as u64, 1));
        let handle = ExtHandle::new(root_inode, root_blocks);
        let extent = ExtExtent::new(0, root_block, 1);

        let mut ctx = ExtContext::new(handle, existing, extent);
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
            let parent = self.stack.last_mut().expect("Root context missing");
            ops::create_lost_found(
                self.io,
                &mut self.allocator,
                self.meta,
                &mut parent.buf,
                &mut parent.child_dir_count,
                &mut self.used_dirs_per_group,
            )?;
        }

        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        // Allocate inode and block for new dir
        let handle = self
            .allocator
            .allocate(self.io, 1)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let block = handle.blocks.0[0].start as u32;

        // Write "." and ".."
        let mut entries = vec![];
        ExtDirEntry::dot(inode).to_raw_buffer(&mut entries);

        // Parent inode
        let parent_inode = self
            .stack
            .last()
            .map(|c| c.handle.inode)
            .unwrap_or(EXT_ROOT_INODE);
        ExtDirEntry::dotdot(parent_inode).to_raw_buffer(&mut entries);

        let extent = ExtExtent::new(0, block, 1);
        let inode_data = ExtInode::from_attr(
            attr,
            self.meta.block_size as u64,
            if attr.dir { 2 } else { 1 },
            self.meta.block_size.div_ceil(512),
            &[extent],
        );
        let inode_buf = inode_data.to_bytes();

        ops::write_inode(self.io, self.meta, inode, &inode_buf)?;

        // Add entry to parent dir
        let entry = ExtDirEntry::from_attr(inode, name, attr);
        if let Some(parent) = self.stack.last_mut() {
            entry.to_raw_buffer(&mut parent.buf);
        }

        // Push new dir context
        let ctx = ExtContext::new(handle, entries, extent);
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
            .allocate(self.io, blocks_needed)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let blocks = handle.blocks;

        // Write content using streaming
        // We iterate over allocated blocks and copy data chunk by chunk.

        use crate::core::utils::stream_copy::write_stream_to_run_list;

        // ... (in write_file) ...
        // Stream content to disk
        if !blocks.0.is_empty() {
            write_stream_to_run_list(self.io, self.meta, source, &blocks, size)?;
        }

        let inode_data = if self.meta.features.has_extents {
            // Build extents for Ext
            use crate::types::ExtExtent;

            // Use MappedRunList to generate runs with logical offsets
            let mapped_runs = MappedRunList::from_run_list(&blocks, 0);

            // Convert to ExtExtents using the From impl
            let extents: Vec<ExtExtent> = mapped_runs
                .iter()
                .map(|run| ExtExtent::from(*run))
                .collect();

            ExtInode::from_attr(
                attr,
                total_size as u64,
                if attr.dir { 2 } else { 1 },
                (blocks.total_units() as u32) * (block_size.div_ceil(512)),
                &extents,
            )
        } else {
            // Build Block Map for Ext2/3
            use crate::utils::block_map::build_block_map;
            let blocks_vec = blocks.to_units();
            let map = build_block_map(self.io, &mut self.allocator, self.meta, &blocks_vec)?;

            ExtInode::from_attr_block_map(
                attr,
                total_size as u64,
                if attr.dir { 2 } else { 1 },
                (blocks.total_units() as u32) * (block_size.div_ceil(512)),
                &map,
            )
        };
        let inode_buf = inode_data.to_bytes();

        ops::write_inode(self.io, self.meta, inode, &inode_buf)?;

        // Add entry to current dir
        let entry = ExtDirEntry::from_attr(inode, name, attr);
        if let Some(ctx) = self.stack.last_mut() {
            entry.to_raw_buffer(&mut ctx.buf);
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            // Pad directory block so last entry spans to end
            ops::pad_directory_block(&mut ctx.buf, self.meta.block_size as usize);
            // Write to first block. Logic limitation: directory size <= 1 block
            if let Some(run) = ctx.handle.blocks.0.first() {
                self.write_block(run.start as u32, &ctx.buf)?;
            }

            // Re-write this directory's inode with correct link count
            // Link count = 2 (for . and ..) + child_dir_count (subdirs pointing back via ..)
            let links = 2 + ctx.child_dir_count;
            let inode_data = ExtInode::from_attr(
                &FileAttributes::new_dir(),
                self.meta.block_size as u64,
                links,
                self.meta.block_size.div_ceil(512),
                &[ctx.extent],
            );
            ops::write_inode(self.io, self.meta, ctx.handle.inode, &inode_data.to_bytes())?;

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
    use crate::meta::ExtFeatureSet;
    use crate::prelude::*;

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    fn test_injector_scenario(meta: ExtMeta, name: &str) {
        println!("--- Running Scenario: {name} ---");
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        // Format
        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta);

        // Complex Tree with Large File (triggers Indirection/Extents)
        // Block size 4096. 15 blocks = 60KB.
        let large_content = vec![0xEEu8; 15 * 4096];

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
                    name: "large.bin".to_string(),
                    content: large_content.clone(),
                    attr: FileAttributes::new_file(),
                },
            ],
        };

        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Check consistency
        let mut checker = ExtChecker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        // Verify content
        let mut resolver = ExtResolver::new(&mut io, &meta);

        // Check simple file
        let content = resolver
            .read_file("/subdir/hello.txt")
            .expect("read_file hello failed");
        assert_eq!(
            content, b"Hello World!",
            "{name}: Simple file content mismatch"
        );

        // Check large file (Exercises BlockMap vs Extents)
        let read_large = resolver
            .read_file("/large.bin")
            .expect("read_file large failed");
        assert_eq!(
            read_large.len(),
            large_content.len(),
            "{name}: Large file size mismatch"
        );
        assert_eq!(
            read_large, large_content,
            "{name}: Large file content mismatch"
        );

        println!("✓ Scenario {name} Passed");
    }

    #[test]
    fn test_ext_variants() {
        // Ext2 (Block Map, no features)
        test_injector_scenario(ExtMeta::new_ext2(SIZE_BYTES, Some("EXT2")), "Ext2");

        // Ext3 (Block Map, has compat)
        test_injector_scenario(ExtMeta::new_ext3(SIZE_BYTES, Some("EXT3")), "Ext3");

        // Ext (Extents, has all features)
        test_injector_scenario(ExtMeta::new(SIZE_BYTES, Some("EXT")), "Ext");

        // Exotic: Extents disabled but 64bit enabled (Manual construction)
        let mut features = ExtFeatureSet::EXT;
        features.has_extents = false;
        features.has_64bit = true;
        let exotic_meta =
            ExtMeta::new_custom(features, SIZE_BYTES, Some("EXOTIC"), None, 4096, 8192);
        test_injector_scenario(exotic_meta, "Exotic (No Extents, 64bit)");
    }
}
