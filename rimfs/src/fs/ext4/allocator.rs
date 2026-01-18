// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use crate::core::allocator::{FsAllocator, FsAllocatorResult, FsHandle};
use crate::fs::ext4::constant::*;
use crate::fs::ext4::group_layout::GroupLayout;
use crate::fs::ext4::meta::Ext4Meta;

#[derive(Debug, Clone)]
pub struct Ext4Handle {
    pub inode: u32,
    pub blocks: Vec<u32>,
}

impl FsHandle for Ext4Handle {}

impl Ext4Handle {
    pub fn new(inode: u32, blocks: Vec<u32>) -> Self {
        Self { inode, blocks }
    }
}

#[derive(Debug)]
pub struct Ext4BlockAllocator<'p> {
    params: &'p Ext4Meta,
    current_group: usize,
    next_free: u32,
}

impl<'p> Ext4BlockAllocator<'p> {
    pub fn new(params: &'p Ext4Meta) -> Self {
        // next_free is a logical index relative to the first_data_block of the group.
        // It should start at 0.
        Self {
            params,
            current_group: 0,
            next_free: 2, // Skip Root (0) and lost+found (1)
        }
    }

    pub fn global_block_number(&self, group: usize, block_in_group: u32) -> u32 {
        let layout = GroupLayout::compute(self.params, group as u32);
        layout.first_data_block + block_in_group
    }

    /// Returns the number of *data* blocks allocated in a specific group.
    pub fn allocated_in_group(&self, group: usize) -> u32 {
        if group > self.current_group {
            return 0;
        }

        if group == self.current_group {
            return self.next_free;
        }

        // For full previous groups, we need to calculate how many data blocks fit.
        // The allocator fills until global_block_number >= group_end.
        // global = first_data + count
        // group_end = group_start + blocks_per_group
        // count_max = group_start + blocks_per_group - first_data
        let layout = GroupLayout::compute(self.params, group as u32);
        let group_end = layout.group_start + self.params.blocks_per_group;

        group_end.saturating_sub(layout.first_data_block)
    }

    pub fn allocate_blocks_list(&mut self, count: usize) -> Vec<u32> {
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
            self.next_free += 1; // logical increment in group

            blocks.push(block);
        }
        blocks
    }

    pub fn next_blocks(&mut self, count: usize) -> Vec<u32> {
        self.allocate_blocks_list(count)
    }

    // Helper accessors
    pub fn block_size(&self) -> usize {
        self.params.block_size as usize
    }

    pub fn block_offset(&self, block: u32) -> u64 {
        block as u64 * self.params.block_size as u64
    }

    /// Returns the total 'logical' units used across all groups.
    /// Note: This is not a direct block count because groups vary in data capacity.
    pub fn used_units(&self) -> usize {
        // Only used for debugging or rough estimates now.
        // Precise usage should be queried via allocated_in_group.
        (self.current_group * self.params.blocks_per_group as usize) + (self.next_free as usize)
    }
}

#[derive(Debug)]
pub struct Ext4MetadataAllocator {
    next_inode: u32,
    total_inodes: u32,
}

impl Ext4MetadataAllocator {
    pub fn new(total_inodes: u32) -> Self {
        Self {
            // Start after lost+found (inode 11)
            next_inode: EXT4_FIRST_INODE + 1,
            total_inodes,
        }
    }

    pub fn allocate_metadata_id(&mut self) -> u32 {
        let id = self.next_inode;
        self.next_inode += 1;
        id
    }

    pub fn used_metadata(&self) -> usize {
        (self.next_inode - EXT4_FIRST_INODE) as usize
    }

    pub fn total_metadata_count(&self) -> usize {
        self.total_inodes as usize
    }

    // Helper to get allocated count in a group
    pub fn allocated_in_group(&self, group: usize, inodes_per_group: u32) -> u32 {
        let start_inode = group as u32 * inodes_per_group + 1; // 1-based
        // Check overlap with allocated range [EXT4_FIRST_INODE, next_inode)
        // Adjust for 1-based vs 0-based logic carefully.
        // Allocated range is: 1..next_inode.

        let end_inode = start_inode + inodes_per_group; // exclusive

        // We use next_inode as the *next free*, so allocated are < next_inode.
        if self.next_inode <= start_inode {
            0
        } else if self.next_inode >= end_inode {
            inodes_per_group
        } else {
            self.next_inode - start_inode
        }
    }
}

#[derive(Debug)]
pub struct Ext4Allocator<'p> {
    pub blocks: Ext4BlockAllocator<'p>,
    pub meta: Ext4MetadataAllocator,
}

impl<'p> Ext4Allocator<'p> {
    pub fn new(params: &'p Ext4Meta) -> Self {
        Self {
            blocks: Ext4BlockAllocator::new(params),
            meta: Ext4MetadataAllocator::new(params.inode_count),
        }
    }
}

impl<'p> FsAllocator<Ext4Handle> for Ext4Allocator<'p> {
    fn allocate_chain(&mut self, count: usize) -> FsAllocatorResult<Ext4Handle> {
        // Allocate 1 inode + count blocks
        let inode = self.meta.allocate_metadata_id();
        let blks = self.blocks.allocate_blocks_list(count);

        Ok(Ext4Handle {
            inode,
            blocks: blks,
        })
    }

    fn used_units(&self) -> usize {
        self.blocks.used_units()
    }

    fn remaining_units(&self) -> usize {
        // Based on blocks, as that's the primary space
        (self.blocks.params.block_count as usize).saturating_sub(self.blocks.used_units())
    }
}
