// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};

use crate::{
    core::{FsResult, traits::FsMeta, utils::volume::generate_volume_id_128},
    fs::ext4::{constant::*, types::Ext4Superblock},
};
use rimio::{RimIO, RimIOStructExt};

#[derive(Debug, Clone)]
pub struct Ext4Meta {
    pub volume_id: [u8; 16],
    pub volume_label: [u8; 16],
    pub volume_size_bytes: u64,
    pub block_size: u32,
    pub block_count: u32,
    pub inode_count: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub group_count: u32,
    pub first_data_block: u32,
}

impl Ext4Meta {
    pub fn new(size_bytes: u64, volume_label: Option<&str>) -> Self {
        Self::new_custom(
            size_bytes,
            volume_label,
            None,
            EXT4_DEFAULT_BLOCK_SIZE,
            EXT4_DEFAULT_INODES_PER_GROUP,
        )
    }

    pub fn new_custom(
        volume_size_bytes: u64,
        volume_label: Option<&str>,
        volume_id: Option<[u8; 16]>,
        block_size: u32,
        inodes_per_group: u32,
    ) -> Self {
        let volume_id = volume_id.unwrap_or_else(|| generate_volume_id_128().to_le_bytes());
        let block_count = (volume_size_bytes / block_size as u64) as u32;
        let blocks_per_group = EXT4_DEFAULT_BLOCKS_PER_GROUP;

        let group_count = (block_count as u64).div_ceil(blocks_per_group as u64) as u32;

        let inode_count = group_count * inodes_per_group;

        let first_data_block = if block_size > 1024 { 0 } else { 1 };

        let mut volume_label_bytes = [0u8; 16];
        if let Some(label) = volume_label {
            let bytes = label.as_bytes();
            let len = bytes.len().min(16);
            volume_label_bytes[..len].copy_from_slice(&bytes[..len]);
        }

        Self {
            volume_id,
            volume_label: volume_label_bytes,
            volume_size_bytes,
            block_size,
            block_count,
            blocks_per_group,
            group_count,
            inode_count,
            inodes_per_group,
            first_data_block,
        }
    }

    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let sb: Ext4Superblock = io.read_struct(EXT4_SUPERBLOCK_OFFSET)?;

        let block_size = 1024 << sb.s_log_block_size;
        let volume_size_bytes = sb.s_blocks_count_lo as u64 * block_size as u64;

        // Extract label from null-terminated or fixed-size buffer
        let label_end = sb
            .s_volume_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(sb.s_volume_name.len());
        let label_str = core::str::from_utf8(&sb.s_volume_name[..label_end]).ok();

        Ok(Self::new_custom(
            volume_size_bytes,
            label_str,
            Some(sb.s_uuid),
            block_size,
            sb.s_inodes_per_group,
        ))
    }
}

impl FsMeta<u32> for Ext4Meta {
    fn unit_size(&self) -> usize {
        self.block_size as usize
    }

    fn unit_offset(&self, unit: u32) -> u64 {
        unit as u64 * self.block_size as u64
    }

    fn root_unit(&self) -> u32 {
        EXT4_ROOT_INODE
    }

    fn first_data_unit(&self) -> u32 {
        self.first_data_block
    }

    fn last_data_unit(&self) -> u32 {
        self.block_count - 1
    }

    fn total_units(&self) -> usize {
        self.block_count as usize
    }

    fn size_bytes(&self) -> u64 {
        self.volume_size_bytes
    }

    fn label(&self) -> String {
        String::from_utf8_lossy(&self.volume_label)
            .trim_matches(char::from(0))
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ext4_meta_creation() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024; // 32 MB
        let meta = Ext4Meta::new(SIZE_BYTES, Some("TESTEXT4"));

        assert_eq!(meta.volume_size_bytes, SIZE_BYTES, "Size mismatch");
        assert_eq!(
            meta.block_size, EXT4_DEFAULT_BLOCK_SIZE,
            "Block size should be 4096"
        );
        assert_eq!(meta.label(), "TESTEXT4", "Volume label mismatch");

        // Calculate expected block count
        let expected_block_count = (SIZE_BYTES / meta.block_size as u64) as u32;
        assert_eq!(
            meta.block_count, expected_block_count,
            "Block count mismatch"
        );

        println!(
            "✓ EXT4 meta created: size={SIZE_BYTES}, block_count={}",
            meta.block_count
        );
    }

    #[test]
    fn test_ext4_group_count() {
        const SIZE_BYTES: u64 = 64 * 1024 * 1024; // 64 MB
        let meta = Ext4Meta::new(SIZE_BYTES, Some("TEST"));

        let expected_groups =
            (meta.block_count as u64).div_ceil(meta.blocks_per_group as u64) as u32;
        assert_eq!(meta.group_count, expected_groups, "Group count mismatch");

        println!(
            "✓ EXT4 group count: {} groups for {} blocks",
            meta.group_count, meta.block_count
        );
    }

    #[test]
    fn test_ext4_sparse_super_groups() {
        use crate::fs::ext4::utils::is_sparse_super_group;

        // Group 0 is always sparse super
        assert!(is_sparse_super_group(0), "Group 0 should be sparse super");
        // Group 1 is always sparse super
        assert!(is_sparse_super_group(1), "Group 1 should be sparse super");
        // Group 3 (3^1) is sparse super
        assert!(is_sparse_super_group(3), "Group 3 should be sparse super");
        // Group 5 (5^1) is sparse super
        assert!(is_sparse_super_group(5), "Group 5 should be sparse super");
        // Group 7 (7^1) is sparse super
        assert!(is_sparse_super_group(7), "Group 7 should be sparse super");
        // Group 9 (3^2) is sparse super
        assert!(is_sparse_super_group(9), "Group 9 should be sparse super");
        // Group 25 (5^2) is sparse super
        assert!(is_sparse_super_group(25), "Group 25 should be sparse super");
        // Group 49 (7^2) is sparse super
        assert!(is_sparse_super_group(49), "Group 49 should be sparse super");

        // Non-sparse groups
        assert!(
            !is_sparse_super_group(2),
            "Group 2 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(4),
            "Group 4 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(6),
            "Group 6 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(8),
            "Group 8 should NOT be sparse super"
        );
        assert!(
            !is_sparse_super_group(10),
            "Group 10 should NOT be sparse super"
        );

        println!("✓ Sparse super group detection verified");
    }

    #[test]
    fn test_ext4_inode_allocation() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = Ext4Meta::new(SIZE_BYTES, Some("INODES"));

        // Total inodes = group_count * inodes_per_group
        let expected_inodes = meta.group_count * meta.inodes_per_group;
        assert_eq!(
            meta.inode_count, expected_inodes,
            "Total inode count mismatch"
        );

        // Verify inodes per group is reasonable
        assert!(meta.inodes_per_group > 0, "Inodes per group should be > 0");
        assert!(
            meta.inodes_per_group <= meta.blocks_per_group * 16,
            "Too many inodes per group"
        );

        println!(
            "✓ EXT4 inode allocation: {} inodes, {} per group",
            meta.inode_count, meta.inodes_per_group
        );
    }

    #[test]
    fn test_ext4_fs_meta_trait() {
        use crate::core::traits::FsMeta;

        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = Ext4Meta::new(SIZE_BYTES, Some("FSMETA"));

        assert_eq!(
            meta.unit_size(),
            meta.block_size as usize,
            "unit_size mismatch"
        );
        assert_eq!(
            meta.root_unit(),
            EXT4_ROOT_INODE,
            "root_unit should be root inode"
        );
        assert_eq!(
            meta.total_units(),
            meta.block_count as usize,
            "total_units mismatch"
        );
        assert_eq!(meta.size_bytes(), SIZE_BYTES, "size_bytes mismatch");

        // Test unit_offset
        let offset = meta.unit_offset(0);
        assert_eq!(offset, 0, "Block 0 offset should be 0");

        let offset_1 = meta.unit_offset(1);
        assert_eq!(
            offset_1, meta.block_size as u64,
            "Block 1 offset should be block_size"
        );

        println!("✓ FsMeta trait implementation verified");
    }
}
