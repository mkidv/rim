// SPDX-License-Identifier: MIT

use crate::fs::ext4::{constant::*, meta::Ext4Meta};

/// Struct representing the layout of an EXT4 block group
#[derive(Debug, Clone, Copy)]
pub struct GroupLayout {
    pub group_id: u32,
    pub group_start: u32,        // Start of the group in the storage space
    pub block_bitmap_block: u32, // Block where the block bitmap is located
    pub inode_bitmap_block: u32, // Block where the inode bitmap is located
    pub inode_table_block: u32,  // Block where the inode table starts
    pub inode_table_blocks: u32, // Number of blocks needed for the inode table
    pub first_data_block: u32,   // First data block for this group
    pub reserved_blocks: u32,    // Number of reserved blocks (e.g., for the superblock, BGDT)
}

impl GroupLayout {
    /// Calculate and initialize a `GroupLayout` for a given group
    pub fn compute(params: &Ext4Meta, group_id: u32) -> Self {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;

        // Utility function: reserved for each group
        let reserved_blocks = Self::reserved_blocks_in_group(group_id, params);

        // Calculations of blocks for bitmaps and inode table
        let block_bitmap_block = Self::block_bitmap_block(group_id, params);
        let inode_bitmap_block = Self::inode_bitmap_block(group_id, params);
        let inode_table_block = Self::inode_table_block(group_id, params);
        let inode_table_blocks =
            (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);

        // First data block
        let first_data_block = Self::first_data_block_in_group(params, group_id);

        Self {
            group_id,
            group_start,
            block_bitmap_block,
            inode_bitmap_block,
            inode_table_block,
            inode_table_blocks,
            first_data_block,
            reserved_blocks,
        }
    }
    // Utility functions moved into GroupLayout

    // Calculates reserved blocks in the group (Superblock + BGDT)
    fn reserved_blocks_in_group(group_id: u32, params: &Ext4Meta) -> u32 {
        use crate::fs::ext4::utils::is_sparse_super_group;
        if is_sparse_super_group(group_id) {
            let bgdt_size = params.group_count * EXT4_BGDT_ENTRY_SIZE as u32;
            let bgdt_blocks = bgdt_size.div_ceil(params.block_size);
            1 + bgdt_blocks // SB (1 block) + BGDT blocks
        } else {
            0
        }
    }

    // Returns the block where the block bitmap is stored for this group
    fn block_bitmap_block(group_id: u32, params: &Ext4Meta) -> u32 {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;
        group_start + Self::reserved_blocks_in_group(group_id, params)
    }

    // Returns the block where the inode bitmap is stored for this group
    fn inode_bitmap_block(group_id: u32, params: &Ext4Meta) -> u32 {
        Self::block_bitmap_block(group_id, params) + 1
    }

    // Returns the block where the inode table starts for this group
    fn inode_table_block(group_id: u32, params: &Ext4Meta) -> u32 {
        Self::inode_bitmap_block(group_id, params) + 1
    }

    // Returns the first data block in the group
    fn first_data_block_in_group(params: &Ext4Meta, group_id: u32) -> u32 {
        let inode_table_blocks =
            (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);
        Self::inode_table_block(group_id, params) + inode_table_blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext4_group_layout_computation() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = Ext4Meta::new(SIZE_BYTES, Some("LAYOUT"));

        for group_id in 0..meta.group_count {
            let layout = GroupLayout::compute(&meta, group_id);

            // Verify group start
            let expected_start = meta.first_data_block + group_id * meta.blocks_per_group;
            assert_eq!(
                layout.group_start, expected_start,
                "Group {group_id}: group_start mismatch"
            );

            // Verify ordering: group_start <= block_bitmap <= inode_bitmap <= inode_table < first_data_block
            assert!(
                layout.block_bitmap_block >= layout.group_start,
                "Group {group_id}: block_bitmap should be >= group_start"
            );
            assert!(
                layout.inode_bitmap_block > layout.block_bitmap_block,
                "Group {group_id}: inode_bitmap should be > block_bitmap"
            );
            assert!(
                layout.inode_table_block > layout.inode_bitmap_block,
                "Group {group_id}: inode_table should be > inode_bitmap"
            );
            assert!(
                layout.first_data_block > layout.inode_table_block,
                "Group {group_id}: first_data_block should be > inode_table"
            );

            println!(
                "✓ Group {group_id}: start={}, bb={}, ib={}, it={}, first_data={}",
                layout.group_start,
                layout.block_bitmap_block,
                layout.inode_bitmap_block,
                layout.inode_table_block,
                layout.first_data_block
            );
        }
    }
}
