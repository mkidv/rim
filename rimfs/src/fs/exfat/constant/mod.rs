// SPDX-License-Identifier: MIT

mod boot_sectors;
mod upcase;

pub use boot_sectors::*;
pub use upcase::*;

pub const EXFAT_OEM_NAME: &[u8; 8] = b"RIM     ";
pub fn oem_name() -> [u8; 8] {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut out = *EXFAT_OEM_NAME; // base
    let ver = VERSION.as_bytes();
    let mut i = 0;
    while i < ver.len() && 3 + i < 8 {
        out[3 + i] = ver[i];
        i += 1;
    }
    out
}

// Disk Layout Parameters

pub const EXFAT_SECTOR_SIZE: u16 = 512;
pub const EXFAT_DEFAULT_CLUSTER_SIZE: u32 = 32768;
pub const EXFAT_SECTORS_PER_CLUSTER: u8 =
    (EXFAT_DEFAULT_CLUSTER_SIZE / EXFAT_SECTOR_SIZE as u32) as u8;
pub const EXFAT_BOUNDARY_ALIGNMENT: u32 = 1024 * 1024; // 1MB alignment
pub const EXFAT_MIN_RESERVED_SECTORS: u16 = 24; // Minimum per spec
pub const EXFAT_BOOT_REGION_SECTORS: u32 = 12;

// FAT Parameters

pub const EXFAT_ENTRY_SIZE: usize = 4;
pub const EXFAT_MEDIA_DESCRIPTOR: u8 = 0xF8;
pub const EXFAT_NUM_FATS: u8 = 1;

// Signature

pub const EXFAT_SIGNATURE: [u8; 2] = [0x55, 0xAA];
pub const EXFAT_FS_NAME: &[u8; 8] = b"EXFAT   ";

// First Cluster
pub const EXFAT_FIRST_CLUSTER: u32 = 2;
pub const EXFAT_BITMAP_CLUSTER: u32 = 2;
pub const EXFAT_UPCASE_CLUSTER: u32 = 3;
pub const EXFAT_ROOT_CLUSTER: u32 = 4;
pub const EXFAT_PADDING: u32 = 3;
pub const EXFAT_EOC: u32 = 0xFFFFFFFF;

// VBR Parameters

pub const EXFAT_VBR_SECTOR: u64 = 0;
pub const EXFAT_VBR_BACKUP_SECTOR: u64 = 12;
pub const EXFAT_VBR_EXTENDED_SECTORS: usize = 8;
pub const EXFAT_VBR_CHECKSUM_SECTOR_INDEX: usize = 11;
pub const EXFAT_BOOT_CODE_SIZE: usize = 390;
pub const EXFAT_JUMP_BOOT: [u8; 3] = [0xEB, 0x76, 0x90]; // BS_jmpBoot

// DirEntry Types

pub const EXFAT_ENTRY_INVAL: u8 = 0x80;
pub const EXFAT_ENTRY_BITMAP: u8 = 0x81;
pub const EXFAT_ENTRY_UPCASE: u8 = 0x82;
pub const EXFAT_ENTRY_LABEL: u8 = 0x83;
pub const EXFAT_ENTRY_GUID: u8 = 0xA0;

pub const EXFAT_ENTRY_PRIMARY: u8 = 0x85;
pub const EXFAT_ENTRY_STREAM: u8 = 0xC0;
pub const EXFAT_ENTRY_NAME: u8 = 0xC1;
pub const EXFAT_EOD: u8 = 0x00;

// Misc

pub const EXFAT_MASK: u32 = 0xFFFFFFFF;
pub const EXFAT_MAX_NAME_UTF16_CHARS: usize = 255;
pub const EXFAT_NAME_ENTRY_CHARS: usize = 15;
pub const EXFAT_NAME_ENTRY_SIZE: usize = 32;
