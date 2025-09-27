// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::fs::fat32::{constant::*, meta::*};

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct Fat32Vbr {
    pub jump_boot: [u8; 3],
    pub oem_name: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub root_entry_count: u16,
    pub total_sectors_16: u16,
    pub media: u8,
    pub fat_size_16: u16,
    pub sectors_per_track: u16,
    pub num_heads: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,

    // FAT32 Extended BPB
    pub fat_size_32: u32,
    pub ext_flags: u16,
    pub fs_version: u16,
    pub root_cluster: u32,
    pub fsinfo_sector: u16,
    pub backup_boot_sector: u16,
    pub reserved: [u8; 12],

    pub drive_number: u8,
    pub reserved1: u8,
    pub boot_signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub fs_type: [u8; 8],

    pub boot_code: [u8; 420],
    pub signature: u16,
}

impl Fat32Vbr {
    pub fn from_meta(meta: &Fat32Meta) -> Self {
        Self {
            jump_boot: FAT_JUMP_BOOT,
            oem_name: oem_name(),
            bytes_per_sector: meta.bytes_per_sector,
            sectors_per_cluster: meta.sectors_per_cluster,
            reserved_sectors: DEFAULT_FAT_RESERVED_SECTORS,
            num_fats: meta.num_fats,
            root_entry_count: FAT_ROOT_ENTRY_COUNT,
            total_sectors_16: FAT_TOTAL_SECTORS_16,
            media: FAT_MEDIA_DESCRIPTOR,
            fat_size_16: FAT_FAT_SIZE_16,
            sectors_per_track: FAT_SECTORS_PER_TRACK,
            num_heads: FAT_HEADS,
            hidden_sectors: FAT_HIDDEN_SECTORS,
            total_sectors_32: meta.volume_size_sectors.min(u32::MAX as u64) as u32,
            fat_size_32: meta.fat_size_sectors,
            ext_flags: FAT_EXT_FLAGS,
            fs_version: FAT_FS_VERSION,
            root_cluster: meta.root_unit(),
            fsinfo_sector: FAT_FSINFO_SECTOR as u16,
            backup_boot_sector: FAT_VBR_BACKUP_SECTOR as u16,
            reserved: [0u8; 12],
            drive_number: FAT_DRIVE_NUMBER,
            reserved1: 0,
            boot_signature: FAT_BOOT_SIGNATURE,
            volume_id: meta.volume_id,
            volume_label: meta.volume_label,
            fs_type: *FAT_FS_TYPE,
            boot_code: [0u8; 420],
            signature: FAT_SIGNATURE,
        }
    }
}

impl Default for Fat32Vbr {
    fn default() -> Self {
        Self {
            jump_boot: FAT_JUMP_BOOT,
            oem_name: oem_name(),
            bytes_per_sector: FAT_SECTOR_SIZE,
            sectors_per_cluster: FAT_SECTORS_PER_CLUSTER,
            reserved_sectors: DEFAULT_FAT_RESERVED_SECTORS,
            num_fats: FAT_NUM_FATS,
            root_entry_count: FAT_ROOT_ENTRY_COUNT,
            total_sectors_16: FAT_TOTAL_SECTORS_16,
            media: FAT_MEDIA_DESCRIPTOR,
            fat_size_16: FAT_FAT_SIZE_16,
            sectors_per_track: FAT_SECTORS_PER_TRACK,
            num_heads: FAT_HEADS,
            hidden_sectors: FAT_HIDDEN_SECTORS,
            total_sectors_32: 0,
            fat_size_32: 0,
            ext_flags: FAT_EXT_FLAGS,
            fs_version: FAT_FS_VERSION,
            root_cluster: FAT_ROOT_CLUSTER,
            fsinfo_sector: FAT_FSINFO_SECTOR as u16,
            backup_boot_sector: FAT_VBR_BACKUP_SECTOR as u16,
            reserved: [0u8; 12],
            drive_number: FAT_DRIVE_NUMBER,
            reserved1: 0,
            boot_signature: FAT_BOOT_SIGNATURE,
            volume_id: 0,
            volume_label: *FAT_VOLUME_LABEL_EMPTY,
            fs_type: *FAT_FS_TYPE,
            boot_code: [0u8; 420],
            signature: FAT_SIGNATURE,
        }
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct Fat32FsInfo {
    pub lead_signature: [u8; 4],
    pub reserved1: [u8; 480],
    pub struct_signature: [u8; 4],
    pub free_cluster_count: u32,
    pub next_free_cluster: u32,
    pub reserved2: [u8; 12],
    pub trail_signature: [u8; 4],
}

impl Fat32FsInfo {
    pub fn from_meta(meta: &Fat32Meta) -> Self {
        Self {
            lead_signature: FAT_FSINFO_LEAD_SIGNATURE,
            reserved1: [0u8; 480],
            struct_signature: FAT_FSINFO_STRUCT_SIGNATURE,
            free_cluster_count: FAT_FSINFO_UNKNOWN,
            next_free_cluster: meta.first_data_unit(),
            reserved2: [0u8; 12],
            trail_signature: FAT_FSINFO_TRAIL_SIGNATURE,
        }
    }
}

impl Default for Fat32FsInfo {
    fn default() -> Self {
        Self {
            lead_signature: FAT_FSINFO_LEAD_SIGNATURE,
            reserved1: [0u8; 480],
            struct_signature: FAT_FSINFO_STRUCT_SIGNATURE,
            free_cluster_count: FAT_FSINFO_UNKNOWN,
            next_free_cluster: FAT_ROOT_CLUSTER + 1,
            reserved2: [0u8; 12],
            trail_signature: FAT_FSINFO_TRAIL_SIGNATURE,
        }
    }
}
