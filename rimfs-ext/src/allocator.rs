// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use rimio::prelude::*;

use crate::core::allocator::{FsAllocator, FsAllocatorResult, FsHandle};
use crate::core::bitmap::{BitmapDriver, BitmapFsMeta};
use crate::core::errors::FsAllocatorError;

use crate::constant::EXT_FIRST_INODE;
use crate::group_layout::GroupLayout;
use crate::meta::ExtMeta;

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
    pub bitmap_block: u32,
}

impl<'a> BitmapFsMeta for ExtBlockBitmap<'a> {
    fn bitmap_offset(&self) -> u64 {
        self.bitmap_block as u64 * self.meta.block_size as u64
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
    allocated_blocks: usize,
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

    pub fn block_offset(&self, block: u32) -> u64 {
        block as u64 * self.params.block_size as u64
    }

    pub fn allocate_blocks_list<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        mut count: usize,
    ) -> FsAllocatorResult<RunList> {
        if count == 0 {
            return Ok(RunList::new());
        }
        let original_count = count;
        let mut list = RunList::new();
        let group_count = self.params.group_count as usize;

        // Simple loop to satisfy allocation across groups
        // We start from last_group to continue where we left off (optimization)
        let start_group = self.last_group;

        for i in 0..group_count {
            let group_idx = (start_group + i) % group_count;

            // Compute layout to find bitmap block
            let layout = GroupLayout::compute(self.params, group_idx as u32);
            let bm_meta = ExtBlockBitmap {
                meta: self.params,
                bitmap_block: layout.block_bitmap_block,
            };

            let mut view = BitmapDriver::new(&bm_meta);

            // Determine hint
            let hint = if group_idx == self.last_group {
                self.last_bit_hint
            } else {
                0
            };

            // Try to find [count] blocks, or whatever fits
            while count > 0 {
                if let Some(bit) = view.find_next_free(io, hint, 1)? {
                    // Limits check: bit must be < blocks_per_group
                    if bit >= self.params.blocks_per_group as u64 {
                        // End of group
                        break;
                    }

                    view.set_bit(io, bit, true)?;

                    // Add to runlist
                    let abs_block = layout.group_start + bit as u32;
                    list.push(Run::new(abs_block as u64, 1));

                    if group_idx < self.allocated_per_group.len() {
                        self.allocated_per_group[group_idx] += 1;
                    }

                    count -= 1;
                    self.last_bit_hint = bit + 1;
                    self.last_group = group_idx;
                } else {
                    // No more space in this group
                    break;
                }
            }

            // Flush changes to this group's bitmap
            view.flush(io)?;

            if count == 0 {
                break;
            }
        }

        if count > 0 {
            // Failed to allocate all needed blocks
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
    pub fn used_units(&self) -> usize {
        self.allocated_blocks
    }
}

/// Adapter for Inode Bitmap in a Group
pub struct ExtInodeBitmap<'a> {
    pub meta: &'a ExtMeta,
    pub bitmap_block: u32,
}

impl<'a> BitmapFsMeta for ExtInodeBitmap<'a> {
    fn bitmap_offset(&self) -> u64 {
        self.bitmap_block as u64 * self.meta.block_size as u64
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
    allocated_inodes: usize,
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

            // In group 0, reserved inodes 1..10 (bits 0..9) must not be allocated to user files
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

                // Inode Number Calculation (1-based)
                let inode = (group_idx as u32 * meta.inodes_per_group) + (bit as u32) + 1;
                return Ok(inode);
            }
        }

        Err(FsAllocatorError::OutOfBlocks)
    }

    pub fn used_metadata(&self) -> usize {
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
}

impl<'p> FsAllocator<ExtHandle> for ExtAllocator<'p> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<ExtHandle> {
        // Allocate 1 inode
        let inode = self.meta.allocate_metadata_id(io, self.blocks.params)?;

        // Allocate blocks
        let blks = self.blocks.allocate_blocks_list(io, count)?;

        Ok(ExtHandle {
            inode,
            blocks: blks,
        })
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<ExtHandle> {
        self.allocate(io, count)
    }

    fn used_units(&self) -> usize {
        self.blocks.used_units()
    }

    fn remaining_units(&self) -> usize {
        (self.blocks.params.block_count as usize).saturating_sub(self.used_units())
    }
}
