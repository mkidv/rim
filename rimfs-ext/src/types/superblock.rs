// SPDX-License-Identifier: MIT
//! EXT Superblock structure

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::constant::*;

/// EXT Superblock structure (1024 bytes)
///
/// This represents the on-disk superblock format for EXT filesystems.
/// Note: This is a simplified version covering the most commonly used fields.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct ExtSuperblock {
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
    pub s_algorithm_usage_bitmap: u32,
    // 0xCC
    pub s_prealloc_blocks: u8,
    pub s_prealloc_dir_blocks: u8,
    pub s_reserved_gdt_blocks: u16,
    // 0xD0
    pub s_journal_uuid: [u8; 16],
    // 0xE0
    pub s_journal_inum: u32,
    pub s_journal_dev: u32,
    pub s_last_orphan: u32,
    // 0xEC
    pub s_hash_seed: [u32; 4],
    // 0xFC
    pub s_def_hash_version: u8,
    pub s_jnl_backup_type: u8,
    pub s_desc_size: u16,
    // 0x100
    pub s_default_mount_opts: u32,
    pub s_first_meta_bg: u32,
    pub s_mkfs_time: u32,
    // 0x10C
    pub s_jnl_blocks: [u32; 17],
    // 0x150
    pub s_blocks_count_hi: u32,
    pub s_r_blocks_count_hi: u32,
    pub s_free_blocks_count_hi: u32,

    pub s_min_extra_isize: u16,
    pub s_want_extra_isize: u16,
    // 0x170
    pub s_flags: u32,
    pub s_raid_stride: u16,
    pub s_mmp_interval: u16,
    pub s_mmp_block: u64,
    // 0x180
    pub s_raid_stripe_width: u32,
    pub s_log_groups_per_flex: u8,
    pub s_checksum_type: u8,
    pub s_reserved_pad: u16,
    pub s_kbytes_written: u64,
    // 0x190
    pub s_snapshot_inum: u32,
    pub s_snapshot_id: u32,
    pub s_snapshot_r_blocks_count: u64,
    pub s_snapshot_list: u32,
    pub s_error_count: u32,
    pub s_first_error_time: u32,
    pub s_first_error_ino: u32,
    pub s_first_error_block: u64,
    pub s_first_error_func: [u8; 32],
    pub s_first_error_line: u32,
    pub s_last_error_time: u32,
    pub s_last_error_ino: u32,
    pub s_last_error_line: u32,
    pub s_last_error_block: u64,
    pub s_last_error_func: [u8; 32],
    pub s_mount_opts: [u8; 64],
    pub s_usr_quota_inum: u32,
    pub s_grp_quota_inum: u32,
    pub s_overhead_clusters: u32,
    pub s_backup_bgs: [u32; 2],
    pub s_encrypt_algos: [u8; 4],
    pub s_encrypt_pw_salt: [u8; 16],
    pub s_lpf_ino: u32,
    pub s_prj_quota_inum: u32,
    pub s_checksum_seed: u32,
    // 0x274
    pub s_reserved: [u8; 392],
    // 0x3FC
    pub s_checksum: u32,
}

impl Default for ExtSuperblock {
    fn default() -> Self {
        // Put creator tag at the end of s_reserved (at offset 0x3F8)
        let mut reserved = [0u8; 392];
        let tag = oem_name();
        reserved[392 - 12..392 - 4].copy_from_slice(&tag);

        Self {
            s_inodes_count: 0,
            s_blocks_count_lo: 0,
            s_r_blocks_count_lo: 0,
            s_free_blocks_count_lo: 0,
            s_free_inodes_count: 0,
            s_first_data_block: 0,
            s_log_block_size: 2, // 4096 bytes
            s_log_cluster_size: 2,
            s_blocks_per_group: EXT_DEFAULT_BLOCKS_PER_GROUP,
            s_clusters_per_group: EXT_DEFAULT_BLOCKS_PER_GROUP,
            s_inodes_per_group: EXT_DEFAULT_INODES_PER_GROUP,
            s_mtime: 0,
            s_wtime: 0,
            s_mnt_count: 0,
            s_max_mnt_count: 0xFFFF,
            s_magic: EXT_SUPERBLOCK_MAGIC,
            s_state: 1,  // Clean
            s_errors: 1, // Continue on errors
            s_minor_rev_level: 0,
            s_lastcheck: 0,
            s_checkinterval: 0,
            s_creator_os: 0, // Linux
            s_rev_level: 1,  // Dynamic
            s_def_resuid: 0,
            s_def_resgid: 0,
            s_first_ino: EXT_FIRST_INODE,
            s_inode_size: EXT_DEFAULT_INODE_SIZE as u16,
            s_block_group_nr: 0,
            s_feature_compat: 0,
            s_feature_incompat: EXT_FEATURE_INCOMPAT_EXTENTS, // Will be updated in from_meta
            s_feature_ro_compat: EXT_FEATURE_RO_COMPAT_SPARSE_SUPER,
            s_uuid: [0; 16],
            s_volume_name: [0; 16],
            s_last_mounted: [0; 64],
            s_algorithm_usage_bitmap: 0,
            s_prealloc_blocks: 0,
            s_prealloc_dir_blocks: 0,
            s_reserved_gdt_blocks: 0,
            s_journal_uuid: [0; 16],
            s_journal_inum: 0,
            s_journal_dev: 0,
            s_last_orphan: 0,
            s_hash_seed: [0; 4],
            s_def_hash_version: 0,
            s_jnl_backup_type: 0,
            s_desc_size: 0,
            s_default_mount_opts: 0,
            s_first_meta_bg: 0,
            s_mkfs_time: 0,
            s_jnl_blocks: [0; 17],
            s_blocks_count_hi: 0,
            s_r_blocks_count_hi: 0,
            s_free_blocks_count_hi: 0,

            s_min_extra_isize: 0,
            s_want_extra_isize: 0,
            s_flags: 0,
            s_raid_stride: 0,
            s_mmp_interval: 0,
            s_mmp_block: 0,
            s_raid_stripe_width: 0,
            s_log_groups_per_flex: 0,
            s_checksum_type: 0,
            s_reserved_pad: 0,
            s_kbytes_written: 0,
            s_snapshot_inum: 0,
            s_snapshot_id: 0,
            s_snapshot_r_blocks_count: 0,
            s_snapshot_list: 0,
            s_error_count: 0,
            s_first_error_time: 0,
            s_first_error_ino: 0,
            s_first_error_block: 0,
            s_first_error_func: [0; 32],
            s_first_error_line: 0,
            s_last_error_time: 0,
            s_last_error_ino: 0,
            s_last_error_line: 0,
            s_last_error_block: 0,
            s_last_error_func: [0; 32],
            s_mount_opts: [0; 64],
            s_usr_quota_inum: 0,
            s_grp_quota_inum: 0,
            s_overhead_clusters: 0,
            s_backup_bgs: [0; 2],
            s_encrypt_algos: [0; 4],
            s_encrypt_pw_salt: [0; 16],
            s_lpf_ino: 0,
            s_prj_quota_inum: 0,
            s_checksum_seed: 0,
            s_reserved: reserved,
            s_checksum: 0,
        }
    }
}

impl ExtSuperblock {
    /// Create a new superblock from filesystem metadata
    pub fn from_meta(meta: &crate::meta::ExtMeta, used_blocks: u32, used_inodes: u32) -> Self {
        let log_block_size = meta.block_size.trailing_zeros() - 10;

        // Volume label
        let mut volume_name = [0u8; 16];
        let label = meta.volume_label.as_bytes();
        let len = label.len().min(16);
        volume_name[..len].copy_from_slice(&label[..len]);

        Self {
            s_inodes_count: meta.inode_count as u32,
            s_blocks_count_lo: meta.block_count as u32,
            s_free_blocks_count_lo: (meta.block_count as u32).saturating_sub(used_blocks),
            s_free_inodes_count: (meta.inode_count as u32).saturating_sub(used_inodes),
            s_first_data_block: meta.first_data_block,
            s_log_block_size: log_block_size,
            s_log_cluster_size: log_block_size,
            s_blocks_per_group: meta.blocks_per_group,
            s_clusters_per_group: meta.blocks_per_group,
            s_inodes_per_group: meta.inodes_per_group,
            // Features
            s_feature_compat: if meta.features.has_compat {
                EXT_FEATURE_COMPAT_EXT_ATTR | EXT_FEATURE_COMPAT_DIR_INDEX
            } else {
                0
            },

            s_feature_incompat: {
                let mut f = EXT_FEATURE_INCOMPAT_FILETYPE;
                if meta.features.has_extents {
                    f |= EXT_FEATURE_INCOMPAT_EXTENTS;
                }
                if meta.features.has_64bit {
                    f |= EXT_FEATURE_INCOMPAT_64BIT;
                }
                f
            },

            s_feature_ro_compat: {
                let mut f = 0;
                if meta.features.has_ro_compat {
                    f |= EXT_FEATURE_RO_COMPAT_LARGE_FILE | EXT_FEATURE_RO_COMPAT_SPARSE_SUPER;
                    // Logic: Ext features
                    if meta.features.has_extents {
                        f |= EXT_FEATURE_RO_COMPAT_DIR_NLINK | EXT_FEATURE_RO_COMPAT_EXTRA_ISIZE;
                    }
                }
                f
            },
            // Set descriptor size to 64 bytes only if 64BIT feature is set
            s_desc_size: if meta.features.has_64bit { 64 } else { 0 },
            s_inode_size: meta.inode_size as u16,
            s_rev_level: if meta.inode_size == 128 { 0 } else { 1 },
            s_first_ino: EXT_FIRST_INODE,
            // UUID
            s_uuid: meta.volume_id,
            s_volume_name: volume_name,
            s_blocks_count_hi: (meta.block_count >> 32) as u32,

            s_free_blocks_count_hi: ((meta.block_count.saturating_sub(used_blocks as u64)) >> 32)
                as u32,
            ..Default::default()
        }
    }

    /// Check if magic is valid
    pub fn is_valid(&self) -> bool {
        self.s_magic == EXT_SUPERBLOCK_MAGIC
    }

    /// Get block size in bytes
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size
    }

    /// Encode to raw bytes
    pub fn to_bytes(&self) -> [u8; EXT_SUPERBLOCK_SIZE] {
        // Safe: ExtSuperblock is exactly EXT_SUPERBLOCK_SIZE bytes by layout and static assert
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("ExtSuperblock size mismatch")
    }
}

// Ensure the struct is exactly 1024 bytes
const _: () = assert!(core::mem::size_of::<ExtSuperblock>() == EXT_SUPERBLOCK_SIZE);
