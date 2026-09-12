// SPDX-License-Identifier: MIT

//! ext2/3/4 block and inode allocator.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use rimio::prelude::*;

use crate::constant::*;
use crate::core::FsInjectorResult;
use crate::core::allocator::{FsAllocator, FsAllocatorResult, FsHandle};
use crate::core::bitmap::{BitmapDriver, BitmapFsMeta, SimpleBitmapMeta};
use crate::core::errors::FsAllocatorError;
use crate::meta::ExtMeta;
use crate::types::GroupLayout;
use zerocopy::IntoBytes;

#[derive(Debug, Clone)]
pub struct ExtHandle {
    pub inode: u32,
    pub blocks: RunList,
}

impl FsHandle for ExtHandle {}

impl ExtHandle {
    pub fn new(inode: u32, blocks: RunList) -> Self {
        Self { inode, blocks }
    }
}

/// Adapter for Block Bitmap in a Group
pub struct ExtBlockBitmap<'a> {
    pub meta: &'a ExtMeta,
    pub bitmap_block: u64,
}

impl<'a> BitmapFsMeta for ExtBlockBitmap<'a> {
    fn bitmap_offset(&self) -> u64 {
        self.bitmap_block * self.meta.block_size as u64
    }
    fn bitmap_size(&self) -> u64 {
        self.meta.block_size as u64
    }
}

#[derive(Debug)]
pub struct Ext4BlockAllocator<'p> {
    pub params: &'p ExtMeta,
    last_group: usize,
    last_bit_hint: u64,
    allocated_blocks: u64,
    allocated_per_group: Vec<u32>,
}

impl<'p> Ext4BlockAllocator<'p> {
    pub fn new(params: &'p ExtMeta) -> Self {
        Self {
            params,
            last_group: 0,
            last_bit_hint: 0,
            allocated_blocks: 0,
            allocated_per_group: vec![0; params.group_count as usize],
        }
    }

    pub fn block_size(&self) -> usize {
        self.params.block_size as usize
    }

    pub fn block_offset(&self, block: u64) -> u64 {
        block * self.params.block_size as u64
    }

    pub fn allocate_blocks_list<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        mut count: u64,
    ) -> FsAllocatorResult<RunList> {
        if count == 0 {
            return Ok(RunList::new());
        }
        let original_count = count;
        let mut list = RunList::new();
        let group_count = self.params.group_count as usize;

        // Simple loop to satisfy allocation across groups
        let start_group = self.last_group;

        for i in 0..group_count {
            let group_idx = (start_group + i) % group_count;

            let layout = GroupLayout::compute(self.params, group_idx as u32);
            let bm_meta = ExtBlockBitmap {
                meta: self.params,
                bitmap_block: layout.block_bitmap_block,
            };

            let mut view = BitmapDriver::new(&bm_meta);

            let mut hint = if group_idx == self.last_group {
                self.last_bit_hint
            } else {
                0
            };

            let max_group_blocks = self.params.blocks_per_group as u64;

            // Try to allocate blocks in coalesced runs
            while count > 0 && hint < max_group_blocks {
                let max_in_group = (max_group_blocks - hint).min(count);
                if max_in_group == 0 {
                    break;
                }

                let mut alloc_chunk = max_in_group;
                let mut found_run = None;

                while alloc_chunk > 0 {
                    if let Some(bit) = view.find_next_free(io, hint, alloc_chunk)?
                        && bit + alloc_chunk <= max_group_blocks
                    {
                        found_run = Some((bit, alloc_chunk));
                        break;
                    }
                    if alloc_chunk == 1 {
                        break;
                    }
                    alloc_chunk = (alloc_chunk / 2).max(1);
                }

                if let Some((bit, run_len)) = found_run {
                    view.set_bits_range(io, bit, run_len, true)?;

                    let abs_block = layout.group_start + bit;
                    list.push(Run::new(abs_block, run_len));

                    if group_idx < self.allocated_per_group.len() {
                        self.allocated_per_group[group_idx] += run_len as u32;
                    }

                    count -= run_len;
                    hint = bit + run_len;
                    self.last_bit_hint = hint;
                    self.last_group = group_idx;
                } else {
                    // No space left in this group
                    break;
                }
            }

            view.flush(io)?;

            if count == 0 {
                break;
            }
        }

        if count > 0 {
            return Err(FsAllocatorError::OutOfBlocks);
        }

        self.allocated_blocks += original_count;
        Ok(list)
    }

    /// Helper for formatting/features to calculate usage
    pub fn allocated_in_group(&self, group: usize) -> u32 {
        let initial_data_blocks = if group == 0 { 2 } else { 0 };
        initial_data_blocks + self.allocated_per_group.get(group).copied().unwrap_or(0)
    }

    // Usage tracking
    pub fn used_units(&self) -> u64 {
        self.allocated_blocks
    }
}

/// Adapter for Inode Bitmap in a Group
pub struct ExtInodeBitmap<'a> {
    pub meta: &'a ExtMeta,
    pub bitmap_block: u64,
}

impl<'a> BitmapFsMeta for ExtInodeBitmap<'a> {
    fn bitmap_offset(&self) -> u64 {
        self.bitmap_block * self.meta.block_size as u64
    }
    fn bitmap_size(&self) -> u64 {
        self.meta.block_size as u64
    }
}

#[derive(Debug)]
pub struct ExtMetadataAllocator {
    pub total_inodes: u64,
    last_group: usize,
    last_inode_hint: u64,
    allocated_inodes: u64,
    allocated_per_group: Vec<u32>,
}

impl ExtMetadataAllocator {
    pub fn new(total_inodes: u64, group_count: usize) -> Self {
        Self {
            total_inodes,
            last_group: 0,
            last_inode_hint: (EXT_FIRST_INODE - 1) as u64,
            allocated_inodes: 0,
            allocated_per_group: vec![0; group_count],
        }
    }

    pub fn allocate_metadata_id<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        meta: &ExtMeta,
    ) -> FsAllocatorResult<u32> {
        let group_count = meta.group_count as usize;
        let start_group = self.last_group;

        for i in 0..group_count {
            let group_idx = (start_group + i) % group_count;
            let layout = GroupLayout::compute(meta, group_idx as u32);
            let bm_meta = ExtInodeBitmap {
                meta,
                bitmap_block: layout.inode_bitmap_block,
            };

            let mut view = BitmapDriver::new(&bm_meta);

            let min_hint = if group_idx == 0 {
                (EXT_FIRST_INODE - 1) as u64
            } else {
                0
            };

            let hint = if group_idx == self.last_group {
                self.last_inode_hint.max(min_hint)
            } else {
                min_hint
            };

            if let Some(bit) = view.find_next_free(io, hint, 1)? {
                if bit >= meta.inodes_per_group as u64 {
                    continue;
                }

                view.set_bit(io, bit, true)?;
                view.flush(io)?;

                if group_idx < self.allocated_per_group.len() {
                    self.allocated_per_group[group_idx] += 1;
                }

                self.last_group = group_idx;
                self.last_inode_hint = bit + 1;
                self.allocated_inodes += 1;

                let inode = (group_idx as u32 * meta.inodes_per_group) + (bit as u32) + 1;
                return Ok(inode);
            }
        }

        Err(FsAllocatorError::OutOfBlocks)
    }

    pub fn used_metadata(&self) -> u64 {
        self.allocated_inodes
    }

    pub fn allocated_in_group(&self, group: usize, _inodes_per_group: u32) -> u32 {
        let initial_used_inodes = if group == 0 { 11 } else { 0 };
        initial_used_inodes + self.allocated_per_group.get(group).copied().unwrap_or(0)
    }
}

#[derive(Debug)]
pub struct ExtAllocator<'p> {
    pub blocks: Ext4BlockAllocator<'p>,
    pub meta: ExtMetadataAllocator,
}

impl<'p> ExtAllocator<'p> {
    pub fn new(params: &'p ExtMeta) -> Self {
        Self {
            blocks: Ext4BlockAllocator::new(params),
            meta: ExtMetadataAllocator::new(params.inode_count, params.group_count as usize),
        }
    }

    /// Flush free block and inode counts to primary and backup superblocks on disk.
    /// Validate the supported mutation layout and restore allocation accounting.
    pub(crate) fn load_existing<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        meta: &ExtMeta,
    ) -> FsInjectorResult<Vec<u16>> {
        let mut dirs = Vec::new();
        for g in 0..meta.group_count() {
            let desc = crate::utils::read_group_descriptor(io, meta, g as u32)?;
            let layout = GroupLayout::compute(meta, g as u32);
            for (lo, hi, expected) in [
                (
                    desc.bg_block_bitmap_lo.get(),
                    desc.bg_block_bitmap_hi.get(),
                    layout.block_bitmap_block,
                ),
                (
                    desc.bg_inode_bitmap_lo.get(),
                    desc.bg_inode_bitmap_hi.get(),
                    layout.inode_bitmap_block,
                ),
                (
                    desc.bg_inode_table_lo.get(),
                    desc.bg_inode_table_hi.get(),
                    layout.inode_table_block,
                ),
            ] {
                let low = u32::from_le(lo) as u64;
                let high = if meta.features.has_64bit {
                    u32::from_le(hi) as u64
                } else {
                    0
                };
                if low | (high << 32) != expected {
                    return Err(crate::core::FsInjectorError::Unsupported(
                        "Mutation of relocated EXT metadata is unsupported",
                    ));
                }
            }
            let block_bm_meta = SimpleBitmapMeta::new(
                layout.block_bitmap_block * meta.block_size as u64,
                meta.block_size as u64,
                meta.group_total_blocks(g) as u64,
            );
            let mut block_driver = BitmapDriver::new(block_bm_meta);
            let used = block_driver
                .count_ones_ro(io)
                .map_err(crate::core::FsInjectorError::IO)? as usize;
            let data_used = used.saturating_sub(layout.metadata_blocks() as usize);
            // allocated_in_group adds the two preformatted directory blocks.
            self.blocks.allocated_per_group[g] =
                data_used.saturating_sub(if g == 0 { 2 } else { 0 }) as u32;

            let inode_bm_meta = SimpleBitmapMeta::new(
                layout.inode_bitmap_block * meta.block_size as u64,
                meta.block_size as u64,
                meta.group_total_inodes(g) as u64,
            );
            let mut inode_driver = BitmapDriver::new(inode_bm_meta);
            let used_inodes = inode_driver
                .count_ones_ro(io)
                .map_err(crate::core::FsInjectorError::IO)? as usize;
            self.meta.allocated_per_group[g] =
                used_inodes.saturating_sub(if g == 0 { 11 } else { 0 }) as u32;
            dirs.push(desc.bg_used_dirs_count_lo.get());
        }
        self.blocks.allocated_blocks = self
            .blocks
            .allocated_per_group
            .iter()
            .map(|n| *n as u64)
            .sum();
        self.meta.allocated_inodes = self
            .meta
            .allocated_per_group
            .iter()
            .map(|n| *n as u64)
            .sum();
        Ok(dirs)
    }

    pub fn flush_superblock<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &ExtMeta,
    ) -> FsInjectorResult {
        flush_superblock(io, self, meta)
    }

    /// Flush updated block group descriptors to primary and backup BGDT locations on disk.
    pub fn flush_bgdt<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &ExtMeta,
        used_dirs_per_group: &[u16],
    ) -> FsInjectorResult {
        flush_bgdt(io, self, meta, used_dirs_per_group)
    }
}

/// Flushes free blocks and inodes count to the primary and backup superblocks.
pub fn flush_superblock<IO: RimIO + ?Sized>(
    io: &mut IO,
    allocator: &ExtAllocator,
    meta: &ExtMeta,
) -> FsInjectorResult {
    let mut total_used_blocks: u64 = 0;
    let mut total_used_inodes: u64 = 0;

    let count = meta.group_count();

    for g in 0..count {
        let layout = GroupLayout::compute(meta, g as u32);
        let metadata_overhead = layout.metadata_blocks();
        let data_used = allocator.blocks.allocated_in_group(g);
        total_used_blocks += (metadata_overhead as u64) + (data_used as u64);

        total_used_inodes += allocator.meta.allocated_in_group(g, meta.inodes_per_group) as u64;
    }

    let free_blocks = meta.block_count.saturating_sub(total_used_blocks);
    let free_inodes = meta.inode_count.saturating_sub(total_used_inodes);

    // Primary Superblock
    io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x0C, free_blocks as u32)?; // s_free_blocks_count_lo
    io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x10, free_inodes as u32)?; // s_free_inodes_count

    // Backup Superblocks
    for group_id in 1..count {
        let has_super =
            !meta.features.has_sparse_super || crate::utils::is_sparse_super_group(group_id as u32);
        if has_super {
            let group_start_block =
                meta.first_data_block as u64 + group_id as u64 * meta.blocks_per_group as u64;
            let sb_copy_offset = group_start_block * meta.block_size as u64;
            io.write_u32_at(sb_copy_offset + 0x0C, free_blocks as u32)?;
            io.write_u32_at(sb_copy_offset + 0x10, free_inodes as u32)?;
        }
    }

    Ok(())
}

/// Flushes updated block group descriptors to primary and backup BGDTs.
pub fn flush_bgdt<IO: RimIO + ?Sized>(
    io: &mut IO,
    allocator: &ExtAllocator,
    meta: &ExtMeta,
    used_dirs_per_group: &[u16],
) -> FsInjectorResult {
    let count = meta.group_count();
    let bgdt_start_offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64;

    let bgdt_entry_size = meta.bgdt_entry_size;
    let mut bgdt_buf = vec![0u8; bgdt_entry_size * count];

    for group_index in 0..count {
        let layout = GroupLayout::compute(meta, group_index as u32);
        let metadata_overhead = layout.metadata_blocks();
        let data_used = allocator.blocks.allocated_in_group(group_index);
        let used_blocks = metadata_overhead + data_used;

        let used_inodes = allocator
            .meta
            .allocated_in_group(group_index, meta.inodes_per_group);

        let free_blocks = (meta.group_total_blocks(group_index) as u32).saturating_sub(used_blocks);
        let free_inodes = (meta.group_total_inodes(group_index) as u32).saturating_sub(used_inodes);

        let default_dirs = if group_index == 0 { 2 } else { 0 };
        let used_dirs = used_dirs_per_group
            .get(group_index)
            .copied()
            .unwrap_or(default_dirs);

        let bgd = crate::types::ExtBlockGroupDesc::new(
            layout.block_bitmap_block,
            layout.inode_bitmap_block,
            layout.inode_table_block,
            free_blocks as u16,
            free_inodes as u16,
            used_dirs,
        );

        let entry_offset = group_index * bgdt_entry_size;
        bgdt_buf[entry_offset..entry_offset + bgdt_entry_size]
            .copy_from_slice(&bgd.as_bytes()[..bgdt_entry_size]);
    }

    io.write_at(bgdt_start_offset, &bgdt_buf)?;

    for group_id in 1..count {
        let layout = GroupLayout::compute(meta, group_id as u32);
        if layout.reserved_blocks > 0 {
            let sb_copy_offset = layout.group_start * (meta.block_size as u64);
            let bgdt_copy_offset = sb_copy_offset.checked_add(meta.block_size as u64).ok_or(
                crate::core::FsInjectorError::Invalid("EXT backup BGDT offset overflow"),
            )?;
            io.write_at(bgdt_copy_offset, &bgdt_buf)?;
        }
    }

    Ok(())
}

impl<'p> FsAllocator<ExtHandle> for ExtAllocator<'p> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<ExtHandle> {
        let inode = self.meta.allocate_metadata_id(io, self.blocks.params)?;

        let blks = self.blocks.allocate_blocks_list(io, count)?;

        Ok(ExtHandle {
            inode,
            blocks: blks,
        })
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<ExtHandle> {
        self.allocate(io, count)
    }

    fn used_units(&self) -> u64 {
        self.blocks.used_units()
    }

    fn remaining_units(&self) -> u64 {
        (self.blocks.params.block_count).saturating_sub(self.used_units())
    }
}
