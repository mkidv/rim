// SPDX-License-Identifier: MIT

//! ext2/3/4 directory tree and extent tree injector.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

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
        types::{ExtDirEntry, ExtExtent, ExtExtentHeader, ExtExtentIndex, ExtInode, ExtLostFound},
        utils,
    },
};
use rimio::prelude::*;
use zerocopy::{FromBytes, IntoBytes};

/// EXT-specific directory context with child subdirectory tracking for link counts
struct ExtContext {
    handle: ExtHandle,
    written_blocks: usize,
    current_block: Vec<u8>,
    /// Number of immediate subdirectories (for parent link count calculation)
    child_dir_count: u16,
    /// Preserved directory attributes (mode, uid, gid, timestamps)
    attr: FileAttributes,
}

/// After an operation fails, discard this injector and inspect/reopen the volume.
/// Errors may leave partial on-disk changes; retrying this instance is prohibited.
pub struct ExtInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: ExtAllocator<'a>,
    meta: &'a ExtMeta,
    stack: Vec<ExtContext>,
    /// Track used directories per group for BGDT
    used_dirs_per_group: Vec<u16>,
    state: FsInjectorState,
}

impl<'a, IO: RimIO + ?Sized> ExtInjector<'a, IO> {
    pub fn new(io: &'a mut IO, params: &'a ExtMeta) -> FsInjectorResult<Self> {
        let mut allocator = ExtAllocator::new(params);
        let used_dirs_per_group = allocator.load_existing(io, params)?;

        Ok(Self {
            io,
            allocator,
            meta: params,
            stack: vec![],
            used_dirs_per_group,
            state: FsInjectorState::default(),
        })
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.allocator.blocks.block_offset(block as u64);
        self.io
            .write_block_best_effort(offset, data, self.meta.block_size as usize)?;
        Ok(())
    }

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        self.mutation(Self::flush_metadata_inner)
    }

    fn flush_metadata_inner(&mut self) -> FsInjectorResult {
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
            let (block_id, finished) = {
                let ctx = self.stack.last_mut().unwrap();
                utils::pad_directory_block(&mut ctx.current_block, block_size);
                let block_units = ctx.handle.blocks.to_units();
                let blk = block_units[ctx.written_blocks];
                ctx.written_blocks += 1;
                let finished = core::mem::take(&mut ctx.current_block);
                (blk, finished)
            };
            self.write_block(block_id, &finished)?;

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

impl<'a, IO: RimIO + ?Sized> FsTreeInjectorBackend for ExtInjector<'a, IO> {
    type Handle = ExtHandle;
    fn injector_state(&mut self) -> &mut FsInjectorState {
        &mut self.state
    }

    fn set_root_context_inner(&mut self, attr: &FileAttributes) -> FsInjectorResult {
        let root_inode = EXT_ROOT_INODE;
        let block_size = self.meta.block_size as usize;
        let (mapping, root_size) = {
            let mut resolver = crate::resolver::ExtResolver::new(self.io, self.meta);
            let inode = resolver.read_inode(root_inode)?;
            let (header, _) = crate::types::ExtInodeHeader::ref_from_prefix(&inode)
                .map_err(|_| FsInjectorError::Invalid("Truncated root inode"))?;
            let size = header.i_size_lo.get() as u64 | ((header.i_size_high.get() as u64) << 32);
            if size == 0 || !size.is_multiple_of(block_size as u64) {
                return Err(FsInjectorError::Invalid("Invalid root directory size"));
            }
            (resolver.inode_extents(&inode, size)?, size)
        };
        let mut root_blocks = RunList::new();
        let mut written_blocks = 0usize;
        let mut packed_entries = Vec::new();
        let mut child_dir_count = 0u16;
        let mut has_lost_found = false;
        let mut logical = 0u64;
        for extent in mapping {
            let physical = extent
                .source_offset
                .ok_or(FsInjectorError::Invalid("Sparse directory"))?;
            if extent.logical_offset != logical
                || extent.len % block_size as u64 != 0
                || physical % block_size as u64 != 0
            {
                return Err(FsInjectorError::Invalid("Invalid directory mapping"));
            }
            for index in 0..extent.len / block_size as u64 {
                let mut existing = vec![0; block_size];
                self.io
                    .read_at(physical + index * block_size as u64, &mut existing)?;
                if root_blocks.total_units() > 0 {
                    utils::pad_directory_block(&mut packed_entries, block_size);
                    let block_units = root_blocks.to_units();
                    let block_id = block_units[written_blocks];
                    written_blocks += 1;
                    self.write_block(block_id, &packed_entries)?;
                    packed_entries.clear();
                }
                root_blocks.push_unit(physical / block_size as u64 + index);
                let mut pos = 0;
                while pos < block_size {
                    if block_size - pos < 8 {
                        return Err(FsInjectorError::Invalid("Truncated directory entry"));
                    }
                    let (header, _) =
                        crate::types::ExtDirEntryHeader::from_record(&existing[pos..])
                            .ok_or(FsInjectorError::Invalid("Invalid directory record length"))?;
                    let ino = header.inode.get();
                    let len = header.rec_len.get() as usize;
                    let name_len = header.name_len as usize;
                    if len < 8
                        || !len.is_multiple_of(4)
                        || len > block_size - pos
                        || name_len > len - 8
                    {
                        return Err(FsInjectorError::Invalid("Invalid directory record length"));
                    }
                    if ino != 0 {
                        let name = &existing[pos + 8..pos + 8 + name_len];
                        if header.file_type == EXT_FT_DIR && name != b"." && name != b".." {
                            child_dir_count = child_dir_count
                                .checked_add(1)
                                .ok_or(FsInjectorError::Invalid("Too many directories"))?;
                        }
                        has_lost_found |= name == b"lost+found";
                        let min_len = (8 + name_len).div_ceil(4) * 4;
                        let start = packed_entries.len();
                        packed_entries.extend_from_slice(&existing[pos..pos + min_len]);
                        let (header, _) = crate::types::ExtDirEntryHeader::mut_from_prefix(
                            &mut packed_entries[start..],
                        )
                        .map_err(|_| {
                            FsInjectorError::Invalid("Truncated packed directory header")
                        })?;
                        header.rec_len = (min_len as u16).into();
                    }
                    pos += len;
                }
            }
            logical += extent.len;
        }
        if logical != root_size {
            return Err(FsInjectorError::Invalid(
                "Incomplete root directory mapping",
            ));
        }
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
            written_blocks,
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

            let mut inode_data = ExtLostFound::create_inode(self.meta.block_size, block);
            if !self.meta.features.has_extents {
                let mut map = crate::types::BlockMapArray::default();
                map.direct[0] = block.into();
                inode_data.set_block_map(&map);
            }
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

    fn write_dir_inner(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let handle = self
            .allocator
            .allocate(self.io, 1)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let block = handle.blocks.0[0].start as u32;

        let mut entries = vec![];
        ExtDirEntry::dot(inode).to_raw_buffer(&mut entries);

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
            written_blocks: 0,
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

    fn write_file_inner(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let block_size = self.meta.block_size;
        let blocks_needed = size.div_ceil(block_size as u64);

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
            let extents = split_extents(&mapped_runs, self.meta.block_size as usize)?;

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
                    eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
                    eh_entries: (extents.len() as u16).into(),
                    eh_max: (max_leaf_extents as u16).into(),
                    eh_depth: 0.into(),
                    eh_generation: 0.into(),
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
                    eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
                    eh_entries: 1.into(),
                    eh_max: 4.into(),
                    eh_depth: 1.into(),
                    eh_generation: 0.into(),
                };
                inode_obj.i_block[0..12].copy_from_slice(root_header.as_bytes());
                let index_entry = ExtExtentIndex::new(extents[0].ee_block.get(), extent_block);
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
            use crate::types::build_block_map;
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

    fn write_symlink_inner(
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
            let block_size = self.meta.block_size as u64;
            let blocks_needed = (target_len as u64).div_ceil(block_size);

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

            let blocks_512 = (blocks.total_units() as u32) * ((block_size as u32).div_ceil(512));

            let inode_data = if self.meta.features.has_extents {
                let mapped_runs = MappedRunList::from_run_list(&blocks, 0);
                let extents = split_extents(&mapped_runs, self.meta.block_size as usize)?;

                ExtInode::from_attr(&symlink_attr, target_len as u64, 1, blocks_512, &extents)
            } else {
                use crate::types::build_block_map;
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

    fn flush_current_inner(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            let block_size = self.meta.block_size as usize;
            // Pad directory block so last entry spans to end
            utils::pad_directory_block(&mut ctx.current_block, block_size);

            let block_units = ctx.handle.blocks.to_units();
            if ctx.written_blocks >= block_units.len() {
                return Err(FsInjectorError::Other(
                    "Mismatch between allocated directory blocks and written blocks",
                ));
            }
            let blk = block_units[ctx.written_blocks];
            self.write_block(blk, &ctx.current_block)?;
            ctx.written_blocks += 1;

            let total_blocks = ctx.written_blocks as u64;
            let total_size = total_blocks * (block_size as u64);
            let blocks_512 =
                (ctx.handle.blocks.total_units() as u32) * (self.meta.block_size.div_ceil(512));
            let links = 2 + ctx.child_dir_count;

            let inode_data = if self.meta.features.has_extents {
                let mapped_runs = MappedRunList::from_run_list(&ctx.handle.blocks, 0);
                let extents = split_extents(&mapped_runs, self.meta.block_size as usize)?;

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
                        eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
                        eh_entries: (extents.len() as u16).into(),
                        eh_max: (max_leaf_extents as u16).into(),
                        eh_depth: 0.into(),
                        eh_generation: 0.into(),
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
                        eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
                        eh_entries: 1.into(),
                        eh_max: 4.into(),
                        eh_depth: 1.into(),
                        eh_generation: 0.into(),
                    };
                    inode_obj.i_block[0..12].copy_from_slice(root_header.as_bytes());
                    let index_entry = ExtExtentIndex::new(extents[0].ee_block.get(), extent_block);
                    inode_obj.i_block[12..24].copy_from_slice(index_entry.as_bytes());
                    inode_obj
                } else {
                    ExtInode::from_attr(&ctx.attr, total_size, links, blocks_512, &extents)
                }
            } else {
                use crate::types::build_block_map;
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

    fn flush_inner(&mut self) -> FsInjectorResult {
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

/// Split initialized extents before narrowing their on-disk fields.
fn split_extents(runs: &MappedRunList, block_size: usize) -> FsInjectorResult<Vec<ExtExtent>> {
    let mut extents = Vec::new();
    let limit = (block_size - 12) / 12;
    for run in runs.iter() {
        let mut done = 0;
        while done < run.physical.length {
            if extents.len() >= limit {
                return Err(FsInjectorError::Unsupported(
                    "Extent tree exceeds supported leaf capacity",
                ));
            }
            let len = (run.physical.length - done).min(32768);
            let logical = run
                .logical_offset
                .checked_add(done)
                .and_then(|v| u32::try_from(v).ok())
                .ok_or(FsInjectorError::Invalid("Extent logical offset overflow"))?;
            let physical = run
                .physical
                .start
                .checked_add(done)
                .filter(|v| *v < (1u64 << 48))
                .ok_or(FsInjectorError::Invalid("Extent physical offset overflow"))?;
            extents.push(ExtExtent::new_48(logical, physical, len as u16));
            done += len;
        }
    }
    Ok(extents)
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;
    use super::*;

    use crate::checker::ExtChecker;
    use crate::core::traits::{FsFormatter, FsTreeResolver};
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

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker
                .check_all()
                .unwrap_or_else(|e| panic!("Checker failed for {}: {:?}", fs_name, e));
            assert_no_errors(&report);

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

    #[test]
    fn root_reopen_preserves_every_block() {
        for meta in [
            ExtMeta::new(32 * 1024 * 1024, None).unwrap(),
            ExtMeta::new_ext2(32 * 1024 * 1024, None).unwrap(),
        ] {
            let mut disk = vec![0; 32 * 1024 * 1024];
            let mut io = MemRimIO::new(&mut disk);
            ExtFormatter::new(&mut io, &meta).format(false).unwrap();
            for pass in 0..2 {
                let mut injector = ExtInjector::new(&mut io, &meta).unwrap();
                injector
                    .set_root_context(&FileAttributes::new_dir())
                    .unwrap();
                for i in pass * 400..(pass + 1) * 400 {
                    let mut source = rimio::SliceRimIO::new(b"payload");
                    injector
                        .write_file(
                            &format!("file-{i:04}"),
                            &mut source,
                            7,
                            &FileAttributes::new_file(),
                        )
                        .unwrap();
                }
                injector.flush().unwrap();
            }
            let mut resolver = ExtResolver::new(&mut io, &meta);
            assert_eq!(resolver.read_dir("/").unwrap().len(), 801);
            for i in 0..800 {
                assert_eq!(
                    resolver.read_file(&format!("file-{i:04}")).unwrap(),
                    b"payload"
                );
            }
        }
    }
    #[test]
    fn root_reopen_fault_sweep_preserves_existing_files() {
        use crate::core::{checker::FsChecker, testing::FaultRimIO};
        for meta in [
            ExtMeta::new(32 * 1024 * 1024, None).unwrap(),
            ExtMeta::new_ext2(32 * 1024 * 1024, None).unwrap(),
        ] {
            let mut baseline = vec![0; 32 * 1024 * 1024];
            {
                let mut io = MemRimIO::new(&mut baseline);
                ExtFormatter::new(&mut io, &meta).format(false).unwrap();
                let mut injector = ExtInjector::new(&mut io, &meta).unwrap();
                injector
                    .set_root_context(&FileAttributes::new_dir())
                    .unwrap();
                for i in 0..400 {
                    injector
                        .write_file(
                            &format!("old-{i:04}"),
                            &mut rimio::SliceRimIO::new(b"old payload"),
                            11,
                            &FileAttributes::new_file(),
                        )
                        .unwrap();
                }
                injector.flush().unwrap();
            }
            let append = |io: &mut FaultRimIO<MemRimIO<'_>>| -> FsInjectorResult {
                let mut injector = ExtInjector::new(io, &meta)?;
                let result = (|| {
                    injector.set_root_context(&FileAttributes::new_dir())?;
                    injector.write_file(
                        "new-file",
                        &mut rimio::SliceRimIO::new(b"new payload"),
                        11,
                        &FileAttributes::new_file(),
                    )?;
                    injector.flush()
                })();
                if result.is_err() {
                    assert!(
                        injector.flush().is_err(),
                        "retry must not hide an earlier failure"
                    );
                    assert!(
                        injector
                            .write_dir("after-error", &FileAttributes::new_dir())
                            .is_err()
                    );
                    assert!(injector.flush_metadata().is_err());
                }
                result
            };
            let mut success = baseline.clone();
            let mut trace = FaultRimIO::new(MemRimIO::new(&mut success), None);
            append(&mut trace).unwrap();
            let operations = trace.operations.clone();
            drop(trace);
            let mut clean_io = MemRimIO::new(&mut success);
            assert!(
                !crate::checker::ExtChecker::new(&mut clean_io, &meta)
                    .check_all()
                    .unwrap()
                    .has_error()
            );
            assert_eq!(
                ExtResolver::new(&mut clean_io, &meta)
                    .read_file("new-file")
                    .unwrap(),
                b"new payload"
            );
            let accounting_matches = |io: &mut MemRimIO<'_>| {
                assert_eq!(meta.group_count, 1);
                let desc = crate::utils::read_group_descriptor(io, &meta, 0).unwrap();
                let mut bitmap = vec![0; meta.block_size as usize];
                let mut free = |block: u32, count: u64| {
                    io.read_at(block as u64 * meta.block_size as u64, &mut bitmap)
                        .unwrap();
                    (0..count)
                        .filter(|&bit| bitmap[bit as usize / 8] & (1 << (bit % 8)) == 0)
                        .count() as u32
                };
                let blocks = free(
                    desc.bg_block_bitmap_lo.get(),
                    meta.block_count - meta.first_data_block as u64,
                );
                let inodes = free(desc.bg_inode_bitmap_lo.get(), meta.inodes_per_group as u64);
                let mut counts = [0; 8];
                io.read_at(1024 + 12, &mut counts).unwrap();
                blocks == desc.bg_free_blocks_count_lo.get() as u32
                    && inodes == desc.bg_free_inodes_count_lo.get() as u32
                    && blocks == u32::from_le_bytes(counts[..4].try_into().unwrap())
                    && inodes == u32::from_le_bytes(counts[4..].try_into().unwrap())
            };
            assert!(accounting_matches(&mut clean_io));
            let mut baseline_io = MemRimIO::new(&mut baseline);
            let baseline_report = crate::checker::ExtChecker::new(&mut baseline_io, &meta)
                .check_all()
                .unwrap();
            let mut mismatches = 0usize;
            let mut findings = 0usize;
            let mut changed = 0usize;
            for (index, operation) in operations.iter().enumerate() {
                let mut disk = baseline.clone();
                let mut fault = FaultRimIO::new(MemRimIO::new(&mut disk), Some(index));
                assert!(
                    append(&mut fault).is_err(),
                    "failure swallowed at {index}: {operation:?}"
                );
                assert!(fault.triggered, "unreached fault {index}: {operation:?}");
                assert_eq!(
                    fault.operations.len(),
                    index + 1,
                    "I/O continued after failure {index}"
                );
                drop(fault);
                changed += usize::from(disk != baseline);
                let mut io = MemRimIO::new(&mut disk);
                let mut resolver = ExtResolver::new(&mut io, &meta);
                let names = resolver.read_dir("/").unwrap();
                for i in 0..400 {
                    let name = format!("old-{i:04}");
                    assert!(
                        names.contains(&name),
                        "lost {name} at {index}: {operation:?}"
                    );
                    assert_eq!(
                        resolver.read_file(&name).unwrap(),
                        b"old payload",
                        "changed {name} at {index}: {operation:?}"
                    );
                }
                drop(resolver);
                let report = crate::checker::ExtChecker::new(&mut io, &meta)
                    .check_all()
                    .unwrap();
                if !accounting_matches(&mut io) {
                    mismatches += 1;
                    #[cfg(feature = "std")]
                    println!("accounting mismatch at {index}: {operation:?}");
                }
                let new_findings: Vec<_> = report
                    .findings
                    .iter()
                    .filter(|f| {
                        !matches!(f.sev, crate::core::checker::Severity::Info)
                            && !baseline_report
                                .findings
                                .iter()
                                .any(|old| old.code == f.code && old.msg == f.msg)
                    })
                    .collect();
                if !new_findings.is_empty() {
                    findings += 1;
                    #[cfg(feature = "std")]
                    println!(
                        "fault {index} {operation:?}: {:?}",
                        new_findings
                            .iter()
                            .map(|f| (f.code, &f.msg))
                            .collect::<Vec<_>>()
                    );
                }
                if !accounting_matches(&mut io) {
                    assert!(
                        !new_findings.is_empty(),
                        "accounting mismatch at index {index} ({operation:?}) was not detected by checker"
                    );
                }
            }
            #[cfg(feature = "std")]
            println!(
                "extents={}: {} fault boundaries; {changed} changed images; {findings} cases with new checker warnings/errors; {mismatches} accounting mismatches; 400 old payloads preserved per case",
                meta.features.has_extents,
                operations.len()
            );
            assert_eq!(mismatches, 9, "expected 9 accounting mismatches");
            assert_eq!(
                findings, 9,
                "expected all 9 accounting mismatches to produce checker findings"
            );
            let _ = changed;
        }
    }

    #[test]
    fn failed_root_flush_cannot_report_success_on_retry() {
        use crate::core::testing::FaultRimIO;
        let meta = ExtMeta::new(32 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0; 32 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);
        ExtFormatter::new(&mut io, &meta).format(false).unwrap();
        // Discover the first directory write during flush without assuming offsets.
        let mut probe_disk = disk.clone();
        let mut probe = FaultRimIO::new(MemRimIO::new(&mut probe_disk), None);
        let mut injector = ExtInjector::new(&mut probe, &meta).unwrap();
        injector
            .set_root_context(&FileAttributes::new_dir())
            .unwrap();
        injector.flush().unwrap();
        drop(injector);
        let first_write = probe
            .operations
            .iter()
            .position(|op| matches!(op, crate::core::testing::FaultOperation::Write { .. }))
            .unwrap();
        let mut fault = FaultRimIO::new(MemRimIO::new(&mut disk), Some(first_write));
        let mut injector = ExtInjector::new(&mut fault, &meta).unwrap();
        injector
            .set_root_context(&FileAttributes::new_dir())
            .unwrap();
        assert!(injector.flush().is_err());
        assert!(
            injector.flush().is_err(),
            "failed context was discarded: retry falsely succeeds"
        );
    }

    #[test]
    fn relocated_metadata_is_rejected_before_mutation() {
        let meta = ExtMeta::new(32 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0; 32 * 1024 * 1024];
        {
            let mut io = MemRimIO::new(&mut disk);
            ExtFormatter::new(&mut io, &meta).format(false).unwrap();
            let offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64;
            let mut desc = crate::utils::read_group_descriptor(&mut io, &meta, 0).unwrap();
            let block = desc.bg_inode_table_lo.get();
            desc.bg_inode_table_lo = ((block + 1).to_le()).into();
            io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
                .unwrap();
        }
        let before = disk.clone();
        let mut io = MemRimIO::new(&mut disk);
        assert!(matches!(
            ExtInjector::new(&mut io, &meta),
            Err(FsInjectorError::Unsupported(_))
        ));
        assert_eq!(disk, before);
    }

    #[test]
    fn initialized_runs_are_split_without_truncation() {
        let mut runs = RunList::new();
        runs.push(Run::new(100, 65537));
        let extents = split_extents(&MappedRunList::from_run_list(&runs, 0), 4096).unwrap();
        assert_eq!(extents.iter().map(|e| e.len() as u64).sum::<u64>(), 65537);
        assert!(extents.iter().all(|e| !e.is_uninit()));
        assert_eq!(extents.len(), 3);
    }
}
