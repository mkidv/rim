// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::fs::exfat::{constant::*, meta::*};

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatBootSector {
    pub jump_boot: [u8; 3],
    pub fs_name: [u8; 8],
    pub must_be_zero: [u8; 53],
    pub partition_offset: u64,
    pub volume_length: u64,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub root_dir_cluster: u32,
    pub volume_serial: u32,
    pub fs_revision: u16,
    pub volume_flags: u16,
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
    pub drive_select: u8,
    pub percent_in_use: u8,
    pub reserved: [u8; 7],
    pub boot_code: [u8; 390],
    pub signature: [u8; 2],
}

impl ExFatBootSector {
    pub fn new_from_meta(meta: &ExFatMeta) -> Self {
        Self {
            jump_boot: EXFAT_JUMP_BOOT,
            fs_name: *EXFAT_FS_NAME,
            must_be_zero: [0u8; 53],
            partition_offset: 0, // Unknown
            volume_length: meta.total_sectors as u64,
            fat_offset: (meta.fat_offset / meta.sector_size as u64) as u32,
            fat_length: meta.fat_size,
            cluster_heap_offset: (meta.cluster_heap_offset / meta.sector_size as u64) as u32,
            cluster_count: meta.cluster_count,
            root_dir_cluster: meta.root_unit(),
            volume_serial: meta.volume_id,
            fs_revision: 0x0100,
            volume_flags: 0x0000,
            bytes_per_sector_shift: meta.sector_size.trailing_zeros() as u8,
            sectors_per_cluster_shift: meta.sectors_per_cluster.trailing_zeros() as u8,
            number_of_fats: meta.num_fats,
            drive_select: 0x80,
            percent_in_use: 0xFF, // Unknown
            reserved: [0u8; 7],
            boot_code: [0x00u8; EXFAT_BOOT_CODE_SIZE],
            signature: EXFAT_SIGNATURE,
        }
    }

    pub fn with_partition_offset(mut self, sectors: u64) -> Self {
        self.partition_offset = sectors;
        self
    }

    pub fn with_percent_in_use(mut self, percents: u8) -> Self {
        self.percent_in_use = percents;
        self
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

impl Default for ExFatBootSector {
    fn default() -> Self {
        Self {
            jump_boot: EXFAT_JUMP_BOOT,
            fs_name: *EXFAT_FS_NAME,
            must_be_zero: [0u8; 53],
            partition_offset: 0,
            volume_length: 0,
            fat_offset: 0,
            fat_length: 0,
            cluster_heap_offset: 0,
            cluster_count: 0,
            root_dir_cluster: EXFAT_ROOT_CLUSTER,
            volume_serial: 0,
            fs_revision: 0x0100,
            volume_flags: 0x0000,
            bytes_per_sector_shift: EXFAT_SECTOR_SIZE.trailing_zeros() as u8,
            sectors_per_cluster_shift: EXFAT_SECTORS_PER_CLUSTER.trailing_zeros() as u8,
            number_of_fats: EXFAT_NUM_FATS,
            drive_select: 0x80,
            percent_in_use: 0xFF,
            reserved: [0u8; 7],
            boot_code: [0x90u8; EXFAT_BOOT_CODE_SIZE],
            signature: EXFAT_SIGNATURE,
        }
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatExBootSector {
    pub reserved: [u8; 510],
    pub signature: [u8; 2],
}

impl ExFatExBootSector {
    pub fn new() -> Self {
        Self {
            reserved: [0; 510],
            signature: EXFAT_SIGNATURE,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}
