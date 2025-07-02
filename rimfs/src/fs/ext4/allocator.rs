// SPDX-License-Identifier: MIT
// rimgen/fs/ext4/allocator.rs

use crate::fs::ext4::constant::*;
use crate::fs::ext4::params::Ext4Params;
use crate::fs::ext4::utils::*;
use crate::core::allocator::{FsAllocator, FsMetadataAllocator};

#[derive(Debug)]
pub struct Ext4BlockAllocator<'p> {
    params: &'p Ext4Params,
    current_group: usize,
    next_free: u32,
}

impl<'p> Ext4BlockAllocator<'p> {
    pub fn new(params: &'p Ext4Params) -> Self {
        Self {
            params,
            current_group: 0,
            next_free: first_data_block_in_group(params, 0),
        }
    }

    fn global_block_number(&self, group: usize, block_in_group: u32) -> u32 {
        first_data_block_in_group(self.params, group as u32) + block_in_group
    }
}

impl<'p> FsAllocator<u32> for Ext4BlockAllocator<'p> {
    fn block_size(&self) -> usize {
        self.params.block_size as usize
    }

    fn block_offset(&self, block: u32) -> u64 {
        block as u64 * self.params.block_size as u64
    }

    fn allocate_blocks(&mut self, count: usize) -> Vec<u32> {
        let mut blocks = Vec::with_capacity(count);

        for _ in 0..count {
            let group_start = self.params.first_data_block
                + self.current_group as u32 * self.params.blocks_per_group;
            let group_end = group_start + self.params.blocks_per_group;

            if self.global_block_number(self.current_group, self.next_free) >= group_end {
                self.current_group += 1;
                self.next_free = 0;
            }

            let block = self.global_block_number(self.current_group, self.next_free);
            self.next_free += 1;

            blocks.push(block);
        }

        blocks
    }

    fn used_blocks(&self) -> usize {
        (self.current_group * self.params.blocks_per_group as usize) + (self.next_free as usize)
    }

    fn total_blocks_count(&self) -> usize {
        self.params.block_count as usize
    }
}
