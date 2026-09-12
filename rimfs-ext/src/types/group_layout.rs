// SPDX-License-Identifier: MIT
//! EXT Block Group layout computation and metadata block ranges.

use crate::meta::ExtMeta;

/// Struct representing the layout of an EXT block group
#[derive(Debug, Clone, Copy)]
pub struct GroupLayout {
    pub group_id: u32,
    pub group_start: u64,
    pub block_bitmap_block: u64,
    pub inode_bitmap_block: u64,
    pub inode_table_block: u64,
    pub inode_table_blocks: u32,
    pub first_data_block: u64,
    pub reserved_blocks: u32,
}

impl GroupLayout {
    /// Calculate and initialize a `GroupLayout` for a given group
    pub fn compute(params: &ExtMeta, group_id: u32) -> Self {
        let group_start =
            params.first_data_block as u64 + group_id as u64 * params.blocks_per_group as u64;

        let reserved_blocks = Self::reserved_blocks_in_group(group_id, params);

        // Calculations of blocks for bitmaps and inode table
        let block_bitmap_block = Self::block_bitmap_block(group_id, params);
        let inode_bitmap_block = Self::inode_bitmap_block(group_id, params);
        let inode_table_block = Self::inode_table_block(group_id, params);
        let inode_table_blocks =
            (params.inodes_per_group * params.inode_size / params.block_size).div_ceil(1);

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

    /// Total blocks used for metadata (reserved + bitmaps + inode table) in this group
    pub fn metadata_blocks(&self) -> u32 {
        (self.first_data_block - self.group_start) as u32
    }

    fn reserved_blocks_in_group(group_id: u32, params: &ExtMeta) -> u32 {
        use crate::utils::is_sparse_super_group;

        // If sparse_super is DISABLED, every group has a superblock and BGDT.
        // If ENABLED, only specific groups (0, 1, 3, 5, 7, 3^n, 5^n, 7^n) have them.
        let has_super = !params.features.has_sparse_super || is_sparse_super_group(group_id);

        if has_super {
            let bgdt_size = params.group_count as u64 * params.bgdt_entry_size as u64;
            let bgdt_blocks = bgdt_size.div_ceil(params.block_size as u64);
            let bgdt_blocks = u32::try_from(bgdt_blocks).unwrap_or(u32::MAX);
            1 + bgdt_blocks
        } else {
            0
        }
    }

    // Returns the block where the block bitmap is stored for this group
    fn block_bitmap_block(group_id: u32, params: &ExtMeta) -> u64 {
        let group_start =
            params.first_data_block as u64 + group_id as u64 * params.blocks_per_group as u64;
        group_start + Self::reserved_blocks_in_group(group_id, params) as u64
    }

    // Returns the block where the inode bitmap is stored for this group
    fn inode_bitmap_block(group_id: u32, params: &ExtMeta) -> u64 {
        Self::block_bitmap_block(group_id, params) + 1
    }

    // Returns the block where the inode table starts for this group
    fn inode_table_block(group_id: u32, params: &ExtMeta) -> u64 {
        Self::inode_bitmap_block(group_id, params) + 1
    }

    // Returns the first data block in the group
    fn first_data_block_in_group(params: &ExtMeta, group_id: u32) -> u64 {
        let inode_table_blocks =
            (params.inodes_per_group * params.inode_size / params.block_size).div_ceil(1);
        Self::inode_table_block(group_id, params) + inode_table_blocks as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext4_group_layout_computation() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = ExtMeta::new(SIZE_BYTES, Some("LAYOUT")).unwrap();

        for group_id in 0..meta.group_count {
            let layout = GroupLayout::compute(&meta, group_id);

            let expected_start =
                meta.first_data_block as u64 + group_id as u64 * meta.blocks_per_group as u64;
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
        }
    }
}
