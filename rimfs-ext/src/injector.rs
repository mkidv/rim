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
        injector::*,
        traits::{FileAttributes, NodeKind},
    },
    {
        allocator::{ExtAllocator, ExtHandle},
        constant::*,
        meta::ExtMeta,
        types::{
            ExtDirEntry, ExtExtent, ExtExtentHeader, ExtExtentIndex, ExtInode, ExtLostFound,
            GroupLayout,
        },
        utils,
    },
};
use rimio::prelude::*;
use zerocopy::IntoBytes;

/// EXT-specific directory context with child subdirectory tracking for link counts
struct ExtContext {
    handle: ExtHandle,
    completed_blocks: Vec<Vec<u8>>,
    current_block: Vec<u8>,
    /// Number of immediate subdirectories (for parent link count calculation)
    child_dir_count: u16,
    /// Preserved directory attributes (mode, uid, gid, timestamps)
    attr: FileAttributes,
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
    pub fn new(io: &'a mut IO, params: &'a ExtMeta) -> FsInjectorResult<Self> {
        let allocator = ExtAllocator::new(params);
        let group_count = params.block_count.div_ceil(params.blocks_per_group as u64) as usize;
        let mut used_dirs_per_group = vec![0u16; group_count];
        // Account for Root Directory (inode 2) and Lost+Found (inode 11) in Group 0
        if group_count > 0 {
            used_dirs_per_group[0] = 2;
        }

        Ok(Self {
            io,
            allocator,
            meta: params,
            stack: vec![],
            used_dirs_per_group,
        })
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.allocator.blocks.block_offset(block as u64);
        self.io
            .write_block_best_effort(offset, data, self.meta.block_size as usize)?;
        Ok(())
    }

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        self.allocator.flush_superblock(self.io, self.meta)?;
        self.allocator
            .flush_bgdt(self.io, self.meta, &self.used_dirs_per_group)?;
        Ok(())
    }

    fn append_dir_entry(&mut self, entry: &ExtDirEntry) -> FsInjectorResult {
        let block_size = self.meta.block_size as usize;
        let entry_min_len = entry.min_rec_len() as usize;

        let needs_new_block = {
            let ctx = self
                .stack
                .last()
                .ok_or(FsInjectorError::Other("Directory stack underflow"))?;
            ctx.current_block.len() + entry_min_len > block_size
        };

        if needs_new_block {
            {
                let ctx = self.stack.last_mut().unwrap();
                utils::pad_directory_block(&mut ctx.current_block, block_size);
                let finished = core::mem::take(&mut ctx.current_block);
                ctx.completed_blocks.push(finished);
            }

            let new_runs = self
                .allocator
                .blocks
                .allocate_blocks_list(self.io, 1)
                .map_err(|_| FsInjectorError::Other("Allocation failed for directory block"))?;

            let ctx = self.stack.last_mut().unwrap();
            ctx.handle.blocks.extend(&new_runs);
        }

        let ctx = self.stack.last_mut().unwrap();
        entry.to_raw_buffer(&mut ctx.current_block);
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<ExtHandle> for ExtInjector<'a, IO> {
    fn set_root_context(&mut self, attr: &FileAttributes) -> FsInjectorResult {
        // Use the pre-formatted root inode (inode 2), not allocating a new one.
        // The root directory was already written by the formatter.

        // First, get the root inode info to find its data block
        let root_inode = EXT_ROOT_INODE;

        // Read existing root directory block (the one written by formatter)
        let layout = GroupLayout::compute(self.meta, 0);
        let root_block = layout.first_data_block;

        // Read existing directory content
        let mut existing = vec![0u8; self.meta.block_size as usize];
        let offset = root_block * self.meta.block_size as u64;
        self.io.read_at(offset, &mut existing)?;

        // Reconstruct a packed entries buffer from the existing directory block
        let mut packed_entries = Vec::new();
        let mut child_dir_count = 0u16;
        let mut has_lost_found = false;
        let mut pos = 0usize;

        while pos + 8 <= existing.len() {
            let entry_inode =
                u32::from_le_bytes(existing[pos..pos + 4].try_into().unwrap_or([0; 4]));
            let rec_len =
                u16::from_le_bytes(existing[pos + 4..pos + 6].try_into().unwrap_or([0; 2]))
                    as usize;
            let name_len = existing[pos + 6] as usize;
            let file_type = existing[pos + 7];

            if entry_inode == 0
                || rec_len == 0
                || rec_len > existing.len() - pos
                || pos + 8 + name_len > existing.len()
            {
                break;
            }

            let name = &existing[pos + 8..pos + 8 + name_len];
            if file_type == EXT_FT_DIR && name != b"." && name != b".." {
                child_dir_count += 1;
            }
            if name == b"lost+found" {
                has_lost_found = true;
            }

            let min_rec_len = (8 + name_len).div_ceil(4) * 4;
            packed_entries.extend_from_slice(&entry_inode.to_le_bytes());
            packed_entries.extend_from_slice(&(min_rec_len as u16).to_le_bytes());
            packed_entries.push(name_len as u8);
            packed_entries.push(file_type);
            packed_entries.extend_from_slice(name);
            let written = 8 + name_len;
            if min_rec_len > written {
                packed_entries.extend(core::iter::repeat_n(0, min_rec_len - written));
            }

            pos += rec_len;
        }

        // Create handle for root (using existing block, inode 2)
        let mut root_blocks = RunList::new();
        root_blocks.push(Run::new(root_block, 1));
        let handle = ExtHandle::new(root_inode, root_blocks);

        let mut dir_attr = attr.clone();
        dir_attr.kind = NodeKind::Directory;
        if let Some(m) = dir_attr.mode {
            dir_attr.mode = Some((m & 0o7777) | 0o040000);
        } else {
            dir_attr.mode = Some(0o040755);
        }

        let ctx = ExtContext {
            handle,
            completed_blocks: Vec::new(),
            current_block: packed_entries,
            child_dir_count,
            attr: dir_attr,
        };
        self.stack.push(ctx);

        if !has_lost_found {
            let runs = self
                .allocator
                .blocks
                .allocate_blocks_list(self.io, 1)
                .map_err(|_| FsInjectorError::Other("Allocation failed for lost+found"))?;
            let block = runs.0[0].start as u32;

            let dir_buf = ExtLostFound::create_dir_block(self.meta.block_size as usize);
            let offset = self.allocator.blocks.block_offset(block as u64);
            self.io
                .write_block_best_effort(offset, &dir_buf, self.meta.block_size as usize)?;

            let inode_data = ExtLostFound::create_inode(self.meta.block_size, block);
            utils::write_inode(
                self.io,
                self.meta,
                ExtLostFound::INODE,
                &inode_data.to_bytes(),
            )?;

            let entry = ExtLostFound::entry();
            self.append_dir_entry(&entry)?;
            if let Some(parent) = self.stack.last_mut() {
                parent.child_dir_count += 1;
            }

            if let Some(count) = self.used_dirs_per_group.get_mut(0) {
                *count += 1;
            }
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
            if attr.is_dir() { 2 } else { 1 },
            self.meta.block_size.div_ceil(512),
            &[extent],
        );
        let inode_buf = inode_data.to_bytes();

        utils::write_inode(self.io, self.meta, inode, &inode_buf)?;

        // Add entry to parent dir using append_dir_entry!
        let entry = ExtDirEntry::from_attr(inode, name, attr);
        self.append_dir_entry(&entry)?;

        // Push new dir context
        let ctx = ExtContext {
            handle,
            completed_blocks: Vec::new(),
            current_block: entries,
            child_dir_count: 0,
            attr: attr.clone(),
        };
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
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        // Allocate inode and blocks
        let block_size = self.meta.block_size;
        let blocks_needed = size.div_ceil(block_size as u64) as usize;

        let handle = self
            .allocator
            .allocate(self.io, blocks_needed)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let blocks = handle.blocks;

        use crate::core::utils::stream_copy::write_stream_to_run_list;

        // Stream content to disk
        if !blocks.0.is_empty() {
            write_stream_to_run_list(self.io, self.meta, source, &blocks, size)?;
        }

        let inode_data = if self.meta.features.has_extents {
            let mapped_runs = MappedRunList::from_run_list(&blocks, 0);
            let extents: Vec<ExtExtent> = mapped_runs
                .iter()
                .map(|run| ExtExtent::from(*run))
                .collect();

            if extents.len() > 4 {
                let max_leaf_extents =
                    (block_size as usize - 12) / core::mem::size_of::<ExtExtent>();
                if extents.len() > max_leaf_extents {
                    return Err(FsInjectorError::Other(
                        "Too many extents for single index block",
                    ));
                }
                let extent_blocks = self
                    .allocator
                    .blocks
                    .allocate_blocks_list(self.io, 1)
                    .map_err(|_| FsInjectorError::Other("Failed to allocate extent index block"))?;
                let extent_block = extent_blocks.0[0].start as u32;

                let mut leaf_data = vec![0u8; block_size as usize];
                let leaf_header = ExtExtentHeader {
                    eh_magic: EXT_EXTENT_HEADER_MAGIC,
                    eh_entries: extents.len() as u16,
                    eh_max: max_leaf_extents as u16,
                    eh_depth: 0,
                    eh_generation: 0,
                };
                leaf_data[0..12].copy_from_slice(leaf_header.as_bytes());
                for (i, extent) in extents.iter().enumerate() {
                    let offset = 12 + i * 12;
                    leaf_data[offset..offset + 12].copy_from_slice(extent.as_bytes());
                }
                self.write_block(extent_block, &leaf_data)?;

                let mut inode_obj = ExtInode::from_attr(
                    attr,
                    size,
                    if attr.is_dir() { 2 } else { 1 },
                    ((blocks.total_units() + 1) as u32) * (block_size.div_ceil(512)),
                    &[],
                );
                let root_header = ExtExtentHeader {
                    eh_magic: EXT_EXTENT_HEADER_MAGIC,
                    eh_entries: 1,
                    eh_max: 4,
                    eh_depth: 1,
                    eh_generation: 0,
                };
                inode_obj.i_block[0..12].copy_from_slice(root_header.as_bytes());
                let index_entry = ExtExtentIndex::new(extents[0].ee_block, extent_block);
                inode_obj.i_block[12..24].copy_from_slice(index_entry.as_bytes());
                inode_obj
            } else {
                ExtInode::from_attr(
                    attr,
                    size,
                    if attr.is_dir() { 2 } else { 1 },
                    (blocks.total_units() as u32) * (block_size.div_ceil(512)),
                    &extents,
                )
            }
        } else {
            use crate::utils::block_map::build_block_map;
            let blocks_vec = blocks.to_units();
            let map = build_block_map(self.io, &mut self.allocator, self.meta, &blocks_vec)?;

            ExtInode::from_attr_block_map(
                attr,
                size,
                if attr.is_dir() { 2 } else { 1 },
                (blocks.total_units() as u32) * (block_size.div_ceil(512)),
                &map,
            )
        };
        let inode_buf = inode_data.to_bytes();

        utils::write_inode(self.io, self.meta, inode, &inode_buf)?;

        // Add entry to current dir
        let entry = ExtDirEntry::from_attr(inode, name, attr);
        self.append_dir_entry(&entry)?;

        Ok(())
    }

    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let target_bytes = target.as_bytes();
        let target_len = target_bytes.len();

        let mut symlink_attr = attr.clone();
        symlink_attr.kind = NodeKind::Symlink;

        if target_len < 60 {
            // Fast symlink (< 60 bytes): target stored in i_block, 0 data blocks allocated
            let handle = self
                .allocator
                .allocate(self.io, 0)
                .map_err(|_| FsInjectorError::Other("Inode allocation failed"))?;
            let inode = handle.inode;

            let inode_data = ExtInode::new_fast_symlink(&symlink_attr, target);
            let inode_buf = inode_data.to_bytes();
            utils::write_inode(self.io, self.meta, inode, &inode_buf)?;

            let entry = ExtDirEntry::from_attr(inode, name, &symlink_attr);
            self.append_dir_entry(&entry)?;
        } else {
            // Slow symlink (>= 60 bytes): allocate data block(s) and stream target payload
            let block_size = self.meta.block_size;
            let blocks_needed = (target_len as u32).div_ceil(block_size) as usize;

            let handle = self
                .allocator
                .allocate(self.io, blocks_needed)
                .map_err(|_| FsInjectorError::Other("Block allocation failed"))?;
            let inode = handle.inode;
            let blocks = handle.blocks;

            let mut target_data = target_bytes.to_vec();
            let mut target_io = rimio::prelude::MemRimIO::new(&mut target_data);
            use crate::core::utils::stream_copy::write_stream_to_run_list;
            write_stream_to_run_list(
                self.io,
                self.meta,
                &mut target_io,
                &blocks,
                target_len as u64,
            )?;

            let blocks_512 = (blocks.total_units() as u32) * (block_size.div_ceil(512));

            let inode_data = if self.meta.features.has_extents {
                let mapped_runs = MappedRunList::from_run_list(&blocks, 0);
                let extents: Vec<ExtExtent> = mapped_runs
                    .iter()
                    .map(|run| ExtExtent::from(*run))
                    .collect();

                ExtInode::from_attr(&symlink_attr, target_len as u64, 1, blocks_512, &extents)
            } else {
                use crate::utils::block_map::build_block_map;
                let blocks_vec = blocks.to_units();
                let map = build_block_map(self.io, &mut self.allocator, self.meta, &blocks_vec)?;

                ExtInode::from_attr_block_map(&symlink_attr, target_len as u64, 1, blocks_512, &map)
            };

            let inode_buf = inode_data.to_bytes();
            utils::write_inode(self.io, self.meta, inode, &inode_buf)?;

            let entry = ExtDirEntry::from_attr(inode, name, &symlink_attr);
            self.append_dir_entry(&entry)?;
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            let block_size = self.meta.block_size as usize;
            // Pad directory block so last entry spans to end
            utils::pad_directory_block(&mut ctx.current_block, block_size);
            ctx.completed_blocks.push(ctx.current_block);

            // Write all directory blocks to disk
            let block_units = ctx.handle.blocks.to_units();
            if block_units.len() != ctx.completed_blocks.len() {
                return Err(FsInjectorError::Other(
                    "Mismatch between allocated directory blocks and written blocks",
                ));
            }
            for (&blk, data) in block_units.iter().zip(ctx.completed_blocks.iter()) {
                self.write_block(blk, data)?;
            }

            let total_blocks = ctx.completed_blocks.len() as u64;
            let total_size = total_blocks * (block_size as u64);
            let blocks_512 =
                (ctx.handle.blocks.total_units() as u32) * (self.meta.block_size.div_ceil(512));
            let links = 2 + ctx.child_dir_count;

            let inode_data = if self.meta.features.has_extents {
                let mapped_runs = MappedRunList::from_run_list(&ctx.handle.blocks, 0);
                let extents: Vec<ExtExtent> = mapped_runs
                    .iter()
                    .map(|run| ExtExtent::from(*run))
                    .collect();

                if extents.len() > 4 {
                    let max_leaf_extents = (block_size - 12) / core::mem::size_of::<ExtExtent>();
                    if extents.len() > max_leaf_extents {
                        return Err(FsInjectorError::Other(
                            "Too many extents for directory index block",
                        ));
                    }
                    let extent_blocks = self
                        .allocator
                        .blocks
                        .allocate_blocks_list(self.io, 1)
                        .map_err(|_| {
                            FsInjectorError::Other("Failed to allocate directory extent block")
                        })?;
                    let extent_block = extent_blocks.0[0].start as u32;

                    let mut leaf_data = vec![0u8; block_size];
                    let leaf_header = ExtExtentHeader {
                        eh_magic: EXT_EXTENT_HEADER_MAGIC,
                        eh_entries: extents.len() as u16,
                        eh_max: max_leaf_extents as u16,
                        eh_depth: 0,
                        eh_generation: 0,
                    };
                    leaf_data[0..12].copy_from_slice(leaf_header.as_bytes());
                    for (i, extent) in extents.iter().enumerate() {
                        let offset = 12 + i * 12;
                        leaf_data[offset..offset + 12].copy_from_slice(extent.as_bytes());
                    }
                    self.write_block(extent_block, &leaf_data)?;

                    let mut inode_obj = ExtInode::from_attr(
                        &ctx.attr,
                        total_size,
                        links,
                        blocks_512 + (self.meta.block_size.div_ceil(512)),
                        &[],
                    );
                    let root_header = ExtExtentHeader {
                        eh_magic: EXT_EXTENT_HEADER_MAGIC,
                        eh_entries: 1,
                        eh_max: 4,
                        eh_depth: 1,
                        eh_generation: 0,
                    };
                    inode_obj.i_block[0..12].copy_from_slice(root_header.as_bytes());
                    let index_entry = ExtExtentIndex::new(extents[0].ee_block, extent_block);
                    inode_obj.i_block[12..24].copy_from_slice(index_entry.as_bytes());
                    inode_obj
                } else {
                    ExtInode::from_attr(&ctx.attr, total_size, links, blocks_512, &extents)
                }
            } else {
                use crate::utils::block_map::build_block_map;
                let blocks_vec = ctx.handle.blocks.to_units();
                let map = build_block_map(self.io, &mut self.allocator, self.meta, &blocks_vec)?;
                ExtInode::from_attr_block_map(&ctx.attr, total_size, links, blocks_512, &map)
            };

            utils::write_inode(self.io, self.meta, ctx.handle.inode, &inode_data.to_bytes())?;

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
    use crate::checker::ExtChecker;
    use crate::formatter::ExtFormatter;
    use crate::meta::ExtFeatureSet;
    use crate::prelude::*;
    use crate::resolver::ExtResolver;
    use rimfs_core::testing::{
        ExpectedFile, ExpectedLink, assert_files, assert_no_errors, assert_symlinks, file_with_attr,
    };

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    fn test_injector_scenario(meta: ExtMeta, _name: &str) {
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let large_content = vec![0xEEu8; 15 * 4096];

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Dir {
                    name: "subdir".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::new_file("hello.txt", b"Hello World!".to_vec())],
                },
                FsNode::new_file("large.bin", large_content.clone()),
            ],
        };

        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        let mut resolver = ExtResolver::new(&mut io, &meta);
        assert_files(
            &mut resolver,
            &[
                ExpectedFile {
                    path: "/subdir/hello.txt",
                    bytes: b"Hello World!",
                },
                ExpectedFile {
                    path: "/large.bin",
                    bytes: &large_content,
                },
            ],
        );
    }

    #[test]
    fn test_ext_variants() {
        test_injector_scenario(ExtMeta::new_ext2(SIZE_BYTES, Some("EXT2")).unwrap(), "Ext2");
        test_injector_scenario(ExtMeta::new_ext3(SIZE_BYTES, Some("EXT3")).unwrap(), "Ext3");
        test_injector_scenario(ExtMeta::new(SIZE_BYTES, Some("EXT")).unwrap(), "Ext");

        let mut features = ExtFeatureSet::EXT;
        features.has_extents = false;
        features.has_64bit = true;
        let exotic_meta =
            ExtMeta::new_custom(features, SIZE_BYTES, Some("EXOTIC"), None, 4096, 8192).unwrap();
        test_injector_scenario(exotic_meta, "Exotic (No Extents, 64bit)");
    }

    #[test]
    fn test_ext_dir_attributes_preserved() {
        let meta = ExtMeta::new(SIZE_BYTES, Some("EXT_DIR_ATTR")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let mut custom_dir_attr = FileAttributes::new_dir();
        custom_dir_attr.mode = Some(0o700);
        custom_dir_attr.uid = Some(1001);
        custom_dir_attr.gid = Some(1002);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::Dir {
                name: "private".to_string(),
                attr: custom_dir_attr,
                children: vec![FsNode::new_file("secret.txt", b"my secret".to_vec())],
            }],
        };

        injector.inject_tree(&mut tree).unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);
        let read_attr = resolver.read_attributes("/private").unwrap();

        assert_eq!(
            read_attr.mode,
            Some(0o700),
            "Directory mode must be preserved across flush"
        );
        assert_eq!(
            read_attr.uid,
            Some(1001),
            "Directory UID must be preserved across flush"
        );
        assert_eq!(
            read_attr.gid,
            Some(1002),
            "Directory GID must be preserved across flush"
        );
    }

    #[test]
    fn test_ext_symlink_fast_and_slow() {
        let meta = ExtMeta::new(SIZE_BYTES, Some("EXT_SYMLINK")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let short_target = "usr/bin/demo"; // 12 bytes < 60
        let long_target =
            "this/is/a/very/long/symlink/target/path/that/is/at/least/60/bytes/long/for/testing"; // 82 bytes >= 60
        let dangling_target = "nonexistent-target";

        let mut setuid_attr = FileAttributes::new_file();
        setuid_attr.mode = Some(0o4755);

        let mut setgid_dir_attr = FileAttributes::new_dir();
        setgid_dir_attr.mode = Some(0o2775);

        let mut sticky_attr = FileAttributes::new_dir();
        sticky_attr.mode = Some(0o1777);

        let mut no_access_attr = FileAttributes::new_file();
        no_access_attr.mode = Some(0o0000);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Symlink {
                    name: "short-link".to_string(),
                    target: short_target.to_string(),
                    attr: FileAttributes::new_symlink(),
                },
                FsNode::Symlink {
                    name: "long-link".to_string(),
                    target: long_target.to_string(),
                    attr: FileAttributes::new_symlink(),
                },
                FsNode::Symlink {
                    name: "dangling-link".to_string(),
                    target: dangling_target.to_string(),
                    attr: FileAttributes::new_symlink(),
                },
                FsNode::new_file_from_source(
                    "setuid-demo",
                    alloc::boxed::Box::new(rimio::prelude::VecRimIO::new(
                        b"setuid binary".to_vec(),
                    )),
                    setuid_attr,
                ),
                FsNode::Dir {
                    name: "setgid-dir".to_string(),
                    children: vec![],
                    attr: setgid_dir_attr,
                },
                FsNode::Dir {
                    name: "tmp-like".to_string(),
                    children: vec![],
                    attr: sticky_attr,
                },
                FsNode::new_file_from_source(
                    "no-access",
                    alloc::boxed::Box::new(rimio::prelude::VecRimIO::new(b"".to_vec())),
                    no_access_attr,
                ),
            ],
        };

        injector.inject_tree(&mut tree).unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert_no_errors(&report);

        let mut resolver = ExtResolver::new(&mut io, &meta);
        assert_symlinks(
            &mut resolver,
            &[
                ExpectedLink {
                    path: "/short-link",
                    target: short_target,
                },
                ExpectedLink {
                    path: "/long-link",
                    target: long_target,
                },
                ExpectedLink {
                    path: "/dangling-link",
                    target: dangling_target,
                },
            ],
        );

        let setuid_read = resolver.read_attributes("/setuid-demo").unwrap();
        assert_eq!(setuid_read.mode, Some(0o4755));

        let setgid_read = resolver.read_attributes("/setgid-dir").unwrap();
        assert_eq!(setgid_read.mode, Some(0o2775));

        let tmp_read = resolver.read_attributes("/tmp-like").unwrap();
        assert_eq!(tmp_read.mode, Some(0o1777));

        let no_access_read = resolver.read_attributes("/no-access").unwrap();
        assert_eq!(no_access_read.mode, Some(0o0000));
    }

    #[test]
    fn test_ext_32bit_uid_gid() {
        let meta = ExtMeta::new(SIZE_BYTES, Some("EXT_32BIT_ID")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let mut high_id_attr = FileAttributes::new_file();
        high_id_attr.uid = Some(70000); // 0x11170 -> lo: 0x1170, hi: 0x0001
        high_id_attr.gid = Some(80000); // 0x13880 -> lo: 0x3880, hi: 0x0001
        high_id_attr.mode = Some(0o640);

        let mut high_id_symlink = FileAttributes::new_symlink();
        high_id_symlink.uid = Some(90000);
        high_id_symlink.gid = Some(95000);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                file_with_attr("app.bin", b"app binary", high_id_attr),
                FsNode::Symlink {
                    name: "app.link".to_string(),
                    target: "app.bin".to_string(),
                    attr: high_id_symlink,
                },
            ],
        };

        injector.inject_tree(&mut tree).unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);

        let file_attr = resolver.read_attributes("/app.bin").unwrap();
        assert_eq!(file_attr.uid, Some(70000));
        assert_eq!(file_attr.gid, Some(80000));
        assert_eq!(file_attr.mode, Some(0o640));

        let link_attr = resolver.read_attributes("/app.link").unwrap();
        assert_eq!(link_attr.uid, Some(90000));
        assert_eq!(link_attr.gid, Some(95000));
        assert_symlinks(
            &mut resolver,
            &[ExpectedLink {
                path: "/app.link",
                target: "app.bin",
            }],
        );
    }

    #[test]
    fn test_ext2_ext3_symlinks() {
        for (meta, _name) in [
            (
                ExtMeta::new_ext2(SIZE_BYTES, Some("EXT2_SYM")).unwrap(),
                "Ext2",
            ),
            (
                ExtMeta::new_ext3(SIZE_BYTES, Some("EXT3_SYM")).unwrap(),
                "Ext3",
            ),
        ] {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);

            ExtFormatter::new(&mut io, &meta)
                .format(false)
                .expect("Format failed");

            let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

            let short_target = "bin/sh";
            let long_target =
                "var/lib/docker/overlay2/1234567890abcdef1234567890abcdef1234567890abcdef/merged";

            let mut tree = FsNode::Container {
                attr: FileAttributes::new_dir(),
                children: vec![
                    FsNode::Symlink {
                        name: "sh".to_string(),
                        target: short_target.to_string(),
                        attr: FileAttributes::new_symlink(),
                    },
                    FsNode::Symlink {
                        name: "docker".to_string(),
                        target: long_target.to_string(),
                        attr: FileAttributes::new_symlink(),
                    },
                ],
            };

            injector.inject_tree(&mut tree).unwrap();

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker.check_all().unwrap();
            assert_no_errors(&report);

            let mut resolver = ExtResolver::new(&mut io, &meta);
            assert_symlinks(
                &mut resolver,
                &[
                    ExpectedLink {
                        path: "/sh",
                        target: short_target,
                    },
                    ExpectedLink {
                        path: "/docker",
                        target: long_target,
                    },
                ],
            );
        }
    }

    #[test]
    fn test_ext_multi_block_directory_growth() {
        use alloc::format;

        for (meta, fs_name) in [
            (
                ExtMeta::new(SIZE_BYTES, Some("EXT4_MULTI")).unwrap(),
                "Ext4",
            ),
            (
                ExtMeta::new_ext2(SIZE_BYTES, Some("EXT2_MULTI")).unwrap(),
                "Ext2",
            ),
        ] {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);

            ExtFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap_or_else(|e| panic!("Format failed for {}: {:?}", fs_name, e));

            let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

            // Create 200 files in a subdirectory to exceed a single 1KB/4KB block
            let num_files = 200;
            let mut children = Vec::with_capacity(num_files);
            for i in 0..num_files {
                let name = format!("entry_long_filename_{:04}.txt", i);
                let content = format!("file content payload for item number {:04}\n", i);
                children.push(FsNode::new_file(&name, content.into_bytes()));
            }

            let mut tree = FsNode::Container {
                attr: FileAttributes::new_dir(),
                children: vec![FsNode::Dir {
                    name: "bigdir".to_string(),
                    attr: FileAttributes::new_dir(),
                    children,
                }],
            };

            injector
                .inject_tree(&mut tree)
                .unwrap_or_else(|e| panic!("Injection failed for {}: {:?}", fs_name, e));

            // Verify with ExtChecker
            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker
                .check_all()
                .unwrap_or_else(|e| panic!("Checker failed for {}: {:?}", fs_name, e));
            assert_no_errors(&report);

            // Verify with ExtResolver
            let mut resolver = ExtResolver::new(&mut io, &meta);
            let dir_entries = resolver
                .read_dir("/bigdir")
                .unwrap_or_else(|e| panic!("read_dir failed for {}: {:?}", fs_name, e));

            // Filter out "." and ".." if returned by read_dir
            let file_entries: Vec<_> = dir_entries
                .into_iter()
                .filter(|name| name != "." && name != "..")
                .collect();
            assert_eq!(
                file_entries.len(),
                num_files,
                "File count mismatch in {}",
                fs_name
            );

            // Read a sampling of files across blocks
            for idx in [0, 10, 50, 99, 150, 199] {
                let path = format!("/bigdir/entry_long_filename_{:04}.txt", idx);
                let expected_content = format!("file content payload for item number {:04}\n", idx);
                let data = resolver.read_file(&path).unwrap_or_else(|e| {
                    panic!("read_file failed for {} at {}: {:?}", fs_name, path, e)
                });
                assert_eq!(data, expected_content.as_bytes());
            }
        }
    }
}
