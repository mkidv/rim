// SPDX-License-Identifier: MIT
//! EXT Superblock structure

use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::constant::*;
use crate::types::flags::{ExtCompatFeatures, ExtIncompatFeatures, ExtRoCompatFeatures};

/// EXT Superblock structure (1024 bytes)
///
/// This represents the on-disk superblock format for EXT filesystems.
/// Note: This is a simplified version covering the most commonly used fields.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ExtSuperblock {
    pub s_inodes_count: U32,
    pub s_blocks_count_lo: U32,
    pub s_r_blocks_count_lo: U32,
    pub s_free_blocks_count_lo: U32,
    pub s_free_inodes_count: U32,
    pub s_first_data_block: U32,
    pub s_log_block_size: U32,
    pub s_log_cluster_size: U32,
    pub s_blocks_per_group: U32,
    pub s_clusters_per_group: U32,
    pub s_inodes_per_group: U32,
    pub s_mtime: U32,
    pub s_wtime: U32,
    pub s_mnt_count: U16,
    pub s_max_mnt_count: U16,
    pub s_magic: U16,
    pub s_state: U16,
    pub s_errors: U16,
    pub s_minor_rev_level: U16,
    pub s_lastcheck: U32,
    pub s_checkinterval: U32,
    pub s_creator_os: U32,
    pub s_rev_level: U32,
    pub s_def_resuid: U16,
    pub s_def_resgid: U16,
    pub s_first_ino: U32,
    pub s_inode_size: U16,
    pub s_block_group_nr: U16,
    pub s_feature_compat: U32,
    pub s_feature_incompat: U32,
    pub s_feature_ro_compat: U32,
    pub s_uuid: [u8; 16],
    pub s_volume_name: [u8; 16],
    pub s_last_mounted: [u8; 64],
    pub s_algorithm_usage_bitmap: U32,
    pub s_prealloc_blocks: u8,
    pub s_prealloc_dir_blocks: u8,
    pub s_reserved_gdt_blocks: U16,
    pub s_journal_uuid: [u8; 16],
    pub s_journal_inum: U32,
    pub s_journal_dev: U32,
    pub s_last_orphan: U32,
    pub s_hash_seed: [U32; 4],
    pub s_def_hash_version: u8,
    pub s_jnl_backup_type: u8,
    pub s_desc_size: U16,
    pub s_default_mount_opts: U32,
    pub s_first_meta_bg: U32,
    pub s_mkfs_time: U32,
    pub s_jnl_blocks: [U32; 17],
    pub s_blocks_count_hi: U32,
    pub s_r_blocks_count_hi: U32,
    pub s_free_blocks_count_hi: U32,
    pub s_min_extra_isize: U16,
    pub s_want_extra_isize: U16,
    pub s_flags: U32,
    pub s_raid_stride: U16,
    pub s_mmp_interval: U16,
    pub s_mmp_block: U64,
    pub s_raid_stripe_width: U32,
    pub s_log_groups_per_flex: u8,
    pub s_checksum_type: u8,
    pub s_reserved_pad: U16,
    pub s_kbytes_written: U64,
    pub s_snapshot_inum: U32,
    pub s_snapshot_id: U32,
    pub s_snapshot_r_blocks_count: U64,
    pub s_snapshot_list: U32,
    pub s_error_count: U32,
    pub s_first_error_time: U32,
    pub s_first_error_ino: U32,
    pub s_first_error_block: U64,
    pub s_first_error_func: [u8; 32],
    pub s_first_error_line: U32,
    pub s_last_error_time: U32,
    pub s_last_error_ino: U32,
    pub s_last_error_line: U32,
    pub s_last_error_block: U64,
    pub s_last_error_func: [u8; 32],
    pub s_mount_opts: [u8; 64],
    pub s_usr_quota_inum: U32,
    pub s_grp_quota_inum: U32,
    pub s_overhead_clusters: U32,
    pub s_backup_bgs: [U32; 2],
    pub s_encrypt_algos: [u8; 4],
    pub s_encrypt_pw_salt: [u8; 16],
    pub s_lpf_ino: U32,
    pub s_prj_quota_inum: U32,
    pub s_checksum_seed: U32,
    pub s_reserved: [u8; 392],
    pub s_checksum: U32,
}

impl Default for ExtSuperblock {
    fn default() -> Self {
        // Put creator tag at the end of s_reserved (at offset 0x3F8)
        let mut reserved = [0u8; 392];
        let tag = oem_name();
        reserved[392 - 12..392 - 4].copy_from_slice(&tag);

        Self {
            s_inodes_count: 0.into(),
            s_blocks_count_lo: 0.into(),
            s_r_blocks_count_lo: 0.into(),
            s_free_blocks_count_lo: 0.into(),
            s_free_inodes_count: 0.into(),
            s_first_data_block: 0.into(),
            s_log_block_size: 2.into(),
            s_log_cluster_size: 2.into(),
            s_blocks_per_group: EXT_DEFAULT_BLOCKS_PER_GROUP.into(),
            s_clusters_per_group: EXT_DEFAULT_BLOCKS_PER_GROUP.into(),
            s_inodes_per_group: EXT_DEFAULT_INODES_PER_GROUP.into(),
            s_mtime: 0.into(),
            s_wtime: 0.into(),
            s_mnt_count: 0.into(),
            s_max_mnt_count: (0xFFFF).into(),
            s_magic: EXT_SUPERBLOCK_MAGIC.into(),
            s_state: 1.into(),  // Clean
            s_errors: 1.into(), // Continue on errors
            s_minor_rev_level: 0.into(),
            s_lastcheck: 0.into(),
            s_checkinterval: 0.into(),
            s_creator_os: 0.into(), // Linux
            s_rev_level: 1.into(),  // Dynamic
            s_def_resuid: 0.into(),
            s_def_resgid: 0.into(),
            s_first_ino: EXT_FIRST_INODE.into(),
            s_inode_size: (EXT_DEFAULT_INODE_SIZE as u16).into(),
            s_block_group_nr: 0.into(),
            s_feature_compat: 0.into(),
            s_feature_incompat: EXT_FEATURE_INCOMPAT_EXTENTS.into(),
            s_feature_ro_compat: EXT_FEATURE_RO_COMPAT_SPARSE_SUPER.into(),
            s_uuid: [0; 16],
            s_volume_name: [0; 16],
            s_last_mounted: [0; 64],
            s_algorithm_usage_bitmap: 0.into(),
            s_prealloc_blocks: 0,
            s_prealloc_dir_blocks: 0,
            s_reserved_gdt_blocks: 0.into(),
            s_journal_uuid: [0; 16],
            s_journal_inum: 0.into(),
            s_journal_dev: 0.into(),
            s_last_orphan: 0.into(),
            s_hash_seed: [U32::new(0); 4],
            s_def_hash_version: 0,
            s_jnl_backup_type: 0,
            s_desc_size: 0.into(),
            s_default_mount_opts: 0.into(),
            s_first_meta_bg: 0.into(),
            s_mkfs_time: 0.into(),
            s_jnl_blocks: [U32::new(0); 17],
            s_blocks_count_hi: 0.into(),
            s_r_blocks_count_hi: 0.into(),
            s_free_blocks_count_hi: 0.into(),
            s_min_extra_isize: 0.into(),
            s_want_extra_isize: 0.into(),
            s_flags: 0.into(),
            s_raid_stride: 0.into(),
            s_mmp_interval: 0.into(),
            s_mmp_block: 0.into(),
            s_raid_stripe_width: 0.into(),
            s_log_groups_per_flex: 0,
            s_checksum_type: 0,
            s_reserved_pad: 0.into(),
            s_kbytes_written: 0.into(),
            s_snapshot_inum: 0.into(),
            s_snapshot_id: 0.into(),
            s_snapshot_r_blocks_count: 0.into(),
            s_snapshot_list: 0.into(),
            s_error_count: 0.into(),
            s_first_error_time: 0.into(),
            s_first_error_ino: 0.into(),
            s_first_error_block: 0.into(),
            s_first_error_func: [0; 32],
            s_first_error_line: 0.into(),
            s_last_error_time: 0.into(),
            s_last_error_ino: 0.into(),
            s_last_error_line: 0.into(),
            s_last_error_block: 0.into(),
            s_last_error_func: [0; 32],
            s_mount_opts: [0; 64],
            s_usr_quota_inum: 0.into(),
            s_grp_quota_inum: 0.into(),
            s_overhead_clusters: 0.into(),
            s_backup_bgs: [U32::new(0); 2],
            s_encrypt_algos: [0; 4],
            s_encrypt_pw_salt: [0; 16],
            s_lpf_ino: 0.into(),
            s_prj_quota_inum: 0.into(),
            s_checksum_seed: 0.into(),
            s_reserved: reserved,
            s_checksum: 0.into(),
        }
    }
}

impl ExtSuperblock {
    /// Create a new superblock from filesystem metadata
    pub fn from_meta(meta: &crate::meta::ExtMeta, used_blocks: u32, used_inodes: u32) -> Self {
        let log_block_size = meta.block_size.trailing_zeros() - 10;

        let mut volume_name = [0u8; 16];
        let label = meta.volume_label.as_bytes();
        let len = label.len().min(16);
        volume_name[..len].copy_from_slice(&label[..len]);

        Self {
            s_inodes_count: (meta.inode_count as u32).into(),
            s_blocks_count_lo: (meta.block_count as u32).into(),
            s_free_blocks_count_lo: ((meta.block_count as u32).saturating_sub(used_blocks)).into(),
            s_free_inodes_count: ((meta.inode_count as u32).saturating_sub(used_inodes)).into(),
            s_first_data_block: meta.first_data_block.into(),
            s_log_block_size: log_block_size.into(),
            s_log_cluster_size: log_block_size.into(),
            s_blocks_per_group: meta.blocks_per_group.into(),
            s_clusters_per_group: meta.blocks_per_group.into(),
            s_inodes_per_group: meta.inodes_per_group.into(),
            // Features
            s_feature_compat: (if meta.features.has_compat {
                (ExtCompatFeatures::EXT_ATTR | ExtCompatFeatures::DIR_INDEX).bits()
            } else {
                0
            })
            .into(),

            s_feature_incompat: ({
                let mut f = ExtIncompatFeatures::FILETYPE;
                if meta.features.has_extents {
                    f |= ExtIncompatFeatures::EXTENTS;
                }
                if meta.features.has_64bit {
                    f |= ExtIncompatFeatures::_64BIT;
                }
                f.bits()
            })
            .into(),

            s_feature_ro_compat: ({
                let mut f = ExtRoCompatFeatures::empty();
                if meta.features.has_ro_compat {
                    f |= ExtRoCompatFeatures::LARGE_FILE | ExtRoCompatFeatures::SPARSE_SUPER;
                    // Logic: Ext features
                    if meta.features.has_extents {
                        f |= ExtRoCompatFeatures::DIR_NLINK | ExtRoCompatFeatures::EXTRA_ISIZE;
                    }
                }
                f.bits()
            })
            .into(),
            // Set descriptor size to 64 bytes only if 64BIT feature is set
            s_desc_size: (if meta.features.has_64bit { 64 } else { 0 }).into(),
            s_inode_size: (meta.inode_size as u16).into(),
            s_rev_level: (if meta.inode_size == 128 { 0 } else { 1 }).into(),
            s_first_ino: EXT_FIRST_INODE.into(),

            s_uuid: meta.volume_id,
            s_volume_name: volume_name,
            s_blocks_count_hi: ((meta.block_count >> 32) as u32).into(),

            s_free_blocks_count_hi: (((meta.block_count.saturating_sub(used_blocks as u64)) >> 32)
                as u32)
                .into(),
            ..Default::default()
        }
    }

    /// Check if magic is valid
    pub fn is_valid(&self) -> bool {
        self.s_magic.get() == EXT_SUPERBLOCK_MAGIC
    }

    /// Get block size in bytes
    pub fn block_size(&self) -> u32 {
        1024 << self.s_log_block_size.get()
    }

    /// Encode to raw bytes
    pub fn to_bytes(&self) -> [u8; EXT_SUPERBLOCK_SIZE] {
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("ExtSuperblock size mismatch")
    }
}

// Ensure the struct is exactly 1024 bytes
const _: () = assert!(core::mem::size_of::<ExtSuperblock>() == EXT_SUPERBLOCK_SIZE);

const _: () = {
    assert!(core::mem::size_of::<ExtSuperblock>() == 1024);
    assert!(core::mem::offset_of!(ExtSuperblock, s_magic) == 56);
    assert!(core::mem::offset_of!(ExtSuperblock, s_feature_incompat) == 96);
};

#[cfg(test)]
mod endian_tests {
    use super::*;
    #[test]
    fn superblock_bytes_and_unaligned_view() {
        let sb = ExtSuperblock {
            s_blocks_count_lo: 0x12345678.into(),
            s_blocks_count_hi: 0x11223344.into(),
            s_checksum: 0xaabbccdd.into(),
            ..Default::default()
        };
        assert_eq!(&sb.as_bytes()[4..8], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(&sb.as_bytes()[336..340], &[0x44, 0x33, 0x22, 0x11]);
        assert_eq!(&sb.as_bytes()[1020..], &[0xdd, 0xcc, 0xbb, 0xaa]);
        let mut bytes = [0; 1025];
        bytes[1..].copy_from_slice(sb.as_bytes());
        let view = ExtSuperblock::ref_from_bytes(&bytes[1..]).unwrap();
        assert!(view.is_valid());
        assert_eq!(view.s_blocks_count_hi.get(), 0x11223344);
        assert!(ExtSuperblock::ref_from_bytes(&bytes[1..1024]).is_err());
    }
}
