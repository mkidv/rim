// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::fs::exfat::{constant::*, meta::*, types::flags::VolumeFlags};

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatBootSector {
    pub jump_boot: [u8; 3],
    pub fs_name: [u8; 8],
    pub reserved: [u8; 53],
    pub partition_offset: u64,
    pub volume_length: u64,
    pub fat_offset: u32,
    pub fat_length: u32,
    pub cluster_heap_offset: u32,
    pub cluster_count: u32,
    pub root_dir_cluster: u32,
    pub volume_serial: u32,
    pub fs_revision: u16,
    pub volume_flags: VolumeFlags,
    pub bytes_per_sector_shift: u8,
    pub sectors_per_cluster_shift: u8,
    pub number_of_fats: u8,
    pub drive_select: u8,
    pub percent_in_use: u8,
    pub reserved1: [u8; 7],
    pub boot_code: [u8; 390],
    pub signature: [u8; 2],
}

impl ExFatBootSector {
    pub fn new_from_meta(meta: &ExFatMeta) -> Self {
        Self {
            jump_boot: EXFAT_JUMP_BOOT,
            fs_name: *EXFAT_FS_NAME,
            reserved: [0u8; 53],
            partition_offset: 0, // Unknown
            volume_length: meta.volume_size_sectors,
            fat_offset: (meta.fat_offset_bytes / meta.bytes_per_sector as u64) as u32,
            fat_length: meta.fat_size_sectors,
            cluster_heap_offset: (meta.cluster_heap_offset_bytes / meta.bytes_per_sector as u64)
                as u32,
            cluster_count: meta.cluster_count,
            root_dir_cluster: meta.root_unit(),
            volume_serial: meta.volume_id,
            fs_revision: 0x0100,
            volume_flags: VolumeFlags::new_volume(), // Clean volume for freshly formatted filesystem
            // VOLUME_DIRTY should only be set during actual usage
            bytes_per_sector_shift: meta.bytes_per_sector.trailing_zeros() as u8,
            sectors_per_cluster_shift: meta.sectors_per_cluster.trailing_zeros() as u8,
            number_of_fats: meta.num_fats,
            drive_select: 0x80,
            percent_in_use: 0xFF, // Unknown
            reserved1: [0u8; 7],
            boot_code: [0xF4u8; EXFAT_BOOT_CODE_SIZE], // HALT instruction (Microsoft recommendation)
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

    pub fn with_volume_flags(mut self, flags: VolumeFlags) -> Self {
        self.volume_flags = flags;
        self
    }

    pub fn mark_volume_dirty(mut self) -> Self {
        self.volume_flags = self.volume_flags.mark_dirty();
        self
    }

    pub fn mark_volume_clean(mut self) -> Self {
        self.volume_flags = self.volume_flags.mark_clean();
        self
    }

    pub fn enable_clear_to_zero(mut self) -> Self {
        self.volume_flags = self.volume_flags.enable_clear_to_zero();
        self
    }

    pub fn is_volume_dirty(&self) -> bool {
        self.volume_flags.is_dirty()
    }

    pub fn has_media_failure(&self) -> bool {
        self.volume_flags.has_media_failure()
    }

    #[inline]
    pub fn neutralize_vbr_volatile(&self) -> ExFatBootSector {
        let mut v = *self;
        v.volume_flags = VolumeFlags::from_bits(0);
        v.percent_in_use = 0;
        v
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
            reserved: [0u8; 53],
            partition_offset: 0,
            volume_length: 0,
            fat_offset: 0,
            fat_length: 0,
            cluster_heap_offset: 0,
            cluster_count: 0,
            root_dir_cluster: EXFAT_ROOT_CLUSTER,
            volume_serial: 0,
            fs_revision: 0x0100,
            volume_flags: VolumeFlags::new_volume(),
            bytes_per_sector_shift: EXFAT_SECTOR_SIZE.trailing_zeros() as u8,
            sectors_per_cluster_shift: EXFAT_SECTORS_PER_CLUSTER.trailing_zeros() as u8,
            number_of_fats: EXFAT_NUM_FATS,
            drive_select: 0x80,
            percent_in_use: 0xFF,
            reserved1: [0u8; 7],
            boot_code: [0xF4u8; EXFAT_BOOT_CODE_SIZE], // HALT instruction
            signature: EXFAT_SIGNATURE,
        }
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatExBootSector {
    pub reserved: [u8; 510], // 512 - 2 = 510
    pub signature: [u8; 2],  // 2 bytes signature 0x55AA like main boot sector
}

impl ExFatExBootSector {
    pub fn new() -> Self {
        let mut reserved = [0u8; 510];
        reserved[..8].copy_from_slice(&oem_name());
        reserved[502..].copy_from_slice(&oem_name());
        Self {
            reserved,
            signature: EXFAT_SIGNATURE,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

impl Default for ExFatExBootSector {
    fn default() -> Self {
        Self::new()
    }
}
