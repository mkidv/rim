// SPDX-License-Identifier: MIT
//! EXT4 Superblock structure

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::fs::ext4::constant::*;

/// EXT4 Superblock structure (1024 bytes)
///
/// This represents the on-disk superblock format for EXT4 filesystems.
/// Note: This is a simplified version covering the most commonly used fields.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct Ext4Superblock {
    // 0x00
    /// Total inode count
    pub s_inodes_count: u32,
    /// Total block count (lower 32 bits)
    pub s_blocks_count_lo: u32,
    /// Reserved block count (lower 32 bits)
    pub s_r_blocks_count_lo: u32,
    /// Free block count (lower 32 bits)
    pub s_free_blocks_count_lo: u32,
    // 0x10
    /// Free inode count
    pub s_free_inodes_count: u32,
    /// First data block
    pub s_first_data_block: u32,
    /// Block size = 1024 << s_log_block_size
    pub s_log_block_size: u32,
    /// Cluster size = 1024 << s_log_cluster_size
    pub s_log_cluster_size: u32,
    // 0x20
    /// Blocks per group
    pub s_blocks_per_group: u32,
    /// Clusters per group
    pub s_clusters_per_group: u32,
    /// Inodes per group
    pub s_inodes_per_group: u32,
    /// Mount time
    pub s_mtime: u32,
    // 0x30
    /// Write time
    pub s_wtime: u32,
    /// Mount count
    pub s_mnt_count: u16,
    /// Max mount count
    pub s_max_mnt_count: u16,
    /// Magic signature (0xEF53)
    pub s_magic: u16,
    /// Filesystem state
    pub s_state: u16,
    /// Behavior on errors
    pub s_errors: u16,
    /// Minor revision level
    pub s_minor_rev_level: u16,
    // 0x40
    /// Time of last check
    pub s_lastcheck: u32,
    /// Max time between checks
    pub s_checkinterval: u32,
    /// Creator OS
    pub s_creator_os: u32,
    /// Revision level
    pub s_rev_level: u32,
    // 0x50
    /// Default reserved UID
    pub s_def_resuid: u16,
    /// Default reserved GID
    pub s_def_resgid: u16,
    /// First non-reserved inode
    pub s_first_ino: u32,
    /// Inode size
    pub s_inode_size: u16,
    /// Block group number of this superblock
    pub s_block_group_nr: u16,
    /// Compatible feature set
    pub s_feature_compat: u32,
    // 0x60
    /// Incompatible feature set
    pub s_feature_incompat: u32,
    /// Read-only compatible feature set
    pub s_feature_ro_compat: u32,
    /// 128-bit UUID for volume
    pub s_uuid: [u8; 16],
    // 0x78
    /// Volume label
    pub s_volume_name: [u8; 16],
    // 0x88
    /// Directory where filesystem was last mounted
    pub s_last_mounted: [u8; 64],
    // 0xC8
    /// For compression (algorithm usage bitmap)
    pub s_algorithm_usage_bitmap: u32,
    // 0xCC
    /// Padding before s_desc_size
    pub s_padding_1: [u8; 50],
    // 0xFE
    /// Size of group descriptors (incompatible feature)
    pub s_desc_size: u16,
    // 0x100
    /// Remaining fields (padding to 1024 bytes)
    pub s_reserved: [u8; 768],
}

impl Default for Ext4Superblock {
    fn default() -> Self {
        Self {
            s_inodes_count: 0,
            s_blocks_count_lo: 0,
            s_r_blocks_count_lo: 0,
            s_free_blocks_count_lo: 0,
            s_free_inodes_count: 0,
            s_first_data_block: 0,
            s_log_block_size: 2, // 4096 bytes
            s_log_cluster_size: 2,
            s_blocks_per_group: EXT4_DEFAULT_BLOCKS_PER_GROUP,
            s_clusters_per_group: EXT4_DEFAULT_BLOCKS_PER_GROUP,
            s_inodes_per_group: EXT4_DEFAULT_INODES_PER_GROUP,
            s_mtime: 0,
            s_wtime: 0,
            s_mnt_count: 0,
            s_max_mnt_count: 0xFFFF,
            s_magic: EXT4_SUPERBLOCK_MAGIC,
            s_state: 1,  // Clean
            s_errors: 1, // Continue on errors
            s_minor_rev_level: 0,
            s_lastcheck: 0,
            s_checkinterval: 0,
            s_creator_os: 0, // Linux
            s_rev_level: 1,  // Dynamic
            s_def_resuid: 0,
            s_def_resgid: 0,
            s_first_ino: EXT4_FIRST_INODE,
            s_inode_size: EXT4_DEFAULT_INODE_SIZE as u16,
            s_block_group_nr: 0,
            s_feature_compat: 0,
            s_feature_incompat: EXT4_FEATURE_INCOMPAT_EXTENTS, // Will be updated in from_meta
            s_feature_ro_compat: EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER,
            s_uuid: [0; 16],
            s_volume_name: [0; 16],
            s_last_mounted: [0; 64],
            s_algorithm_usage_bitmap: 0,
            s_padding_1: [0; 50],
            s_desc_size: 0,
            s_reserved: [0; 768],
        }
    }
}

impl Ext4Superblock {
    /// Create a new superblock from filesystem metadata
    pub fn from_meta(
        meta: &crate::fs::ext4::meta::Ext4Meta,
        used_blocks: u32,
        used_inodes: u32,
    ) -> Self {
        let log_block_size = meta.block_size.trailing_zeros() - 10;

        // Volume label
        let mut volume_name = [0u8; 16];
        let label = meta.volume_label.as_bytes();
        let len = label.len().min(16);
        volume_name[..len].copy_from_slice(&label[..len]);

        Self {
            s_inodes_count: meta.inode_count,
            s_blocks_count_lo: meta.block_count,
            s_free_blocks_count_lo: meta.block_count - used_blocks,
            s_free_inodes_count: meta.inode_count - used_inodes,
            s_first_data_block: meta.first_data_block,
            s_log_block_size: log_block_size,
            s_log_cluster_size: log_block_size,
            s_blocks_per_group: meta.blocks_per_group,
            s_clusters_per_group: meta.blocks_per_group,
            s_inodes_per_group: meta.inodes_per_group,
            // Features
            s_feature_compat: EXT4_FEATURE_COMPAT_EXT_ATTR | EXT4_FEATURE_COMPAT_DIR_INDEX,
            // Enable 64BIT to support 64-byte block group descriptors, and FILETYPE for directory entries
            s_feature_incompat: EXT4_FEATURE_INCOMPAT_EXTENTS
                | EXT4_FEATURE_INCOMPAT_64BIT
                | EXT4_FEATURE_INCOMPAT_FILETYPE,
            s_feature_ro_compat: EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
                | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
                | EXT4_FEATURE_RO_COMPAT_DIR_NLINK
                | EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE,
            // Set descriptor size to 64 bytes
            s_desc_size: 64,
            // UUID
            s_uuid: meta.volume_id,
            s_volume_name: volume_name,
            ..Default::default()
        }
    }

    /// Check if magic is valid
    pub fn is_valid(&self) -> bool {
        self.s_magic == EXT4_SUPERBLOCK_MAGIC
    }

    /// Get block size in bytes
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Encode to raw bytes
    pub fn to_bytes(&self) -> [u8; EXT4_SUPERBLOCK_SIZE] {
        // Safe: Ext4Superblock is exactly EXT4_SUPERBLOCK_SIZE bytes by layout and static assert
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("Ext4Superblock size mismatch")
    }
}

// Ensure the struct is exactly 1024 bytes
const _: () = assert!(core::mem::size_of::<Ext4Superblock>() == EXT4_SUPERBLOCK_SIZE);
