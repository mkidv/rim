// SPDX-License-Identifier: MIT

pub const FAT_OEM_NAME: &[u8; 8] = b"RIM     "; // BS_OEMName

pub fn oem_name() -> [u8; 8] {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut out = *FAT_OEM_NAME; // base
    let ver = VERSION.as_bytes();
    let mut i = 0;
    while i < ver.len() && 3 + i < 8 {
        out[3 + i] = ver[i];
        i += 1;
    }
    out
}

// === Disk Layout Parameters ===

pub const FAT_MAX_SECTOR_SIZE: usize = 4096;
pub const FAT_SECTOR_SIZE: u16 = 512; // BPB_BytsPerSec
pub const FAT_CLUSTER_SIZE: u32 = 4096;
pub const FAT_SECTORS_PER_CLUSTER: u8 = (FAT_CLUSTER_SIZE / FAT_SECTOR_SIZE as u32) as u8;

pub const DEFAULT_FAT_RESERVED_SECTORS: u16 = 32; // BPB_RsvdSecCnt
pub const FAT_NUM_FATS: u8 = 2; // BPB_NumFATs
pub const FAT_HEADS: u16 = 255; // BPB_NumHeads (CHS hint)
pub const FAT_SECTORS_PER_TRACK: u16 = 63; // BPB_SecPerTrk (CHS hint)
pub const FAT_HIDDEN_SECTORS: u32 = 0; // BPB_HiddSec

// === FAT Region Parameters ===

pub const FAT_ENTRY_SIZE: usize = 4;
pub const FAT_MEDIA_DESCRIPTOR: u8 = 0xF8; // BPB_Media
pub const FAT_EOC: u32 = 0x0FFFFFF8;
pub const FAT_FIRST_CLUSTER: u32 = 2;
pub const FAT_ROOT_CLUSTER: u32 = 2; // BPB_RootClus

// === Special Sector Numbers ===

pub const FAT_VBR_SECTOR: u64 = 0;
pub const FAT_VBR_BACKUP_SECTOR: u64 = 6;
pub const FAT_FSINFO_SECTOR: u64 = 1;
pub const FAT_FSINFO_BACKUP_SECTOR: u64 = 7;

// === Standard FAT32 BPB / Extended BPB / FSINFO Constants ===

pub const FAT_JUMP_BOOT: [u8; 3] = [0xEB, 0x58, 0x90]; // BS_jmpBoot
// pub const FAT_OEM_NAME: &[u8; 8] = b"MSWIN4.1"; // BS_OEMName
pub const FAT_ROOT_ENTRY_COUNT: u16 = 0; // BPB_RootEntCnt (always 0 for FAT32)
pub const FAT_TOTAL_SECTORS_16: u16 = 0; // BPB_TotSec16 (always 0 for FAT32)
pub const FAT_FAT_SIZE_16: u16 = 0; // BPB_FATSz16 (always 0 for FAT32)
pub const FAT_EXT_FLAGS: u16 = 0; // BPB_ExtFlags
pub const FAT_FS_VERSION: u16 = 0; // BPB_FSVer
pub const FAT_DRIVE_NUMBER: u8 = 0x80; // BS_DrvNum
pub const FAT_BOOT_SIGNATURE: u8 = 0x29; // BS_BootSig
pub const FAT_SIGNATURE: u16 = 0x55AA;
pub const FAT_FS_TYPE: &[u8; 8] = b"FAT32   "; // BS_FilSysType
pub const FAT_VOLUME_LABEL_EMPTY: &[u8; 11] = b"NO NAME    ";
pub const FAT_FSINFO_LEAD_SIGNATURE: [u8; 4] = [0x52, 0x52, 0x61, 0x41]; // 'R','R','a','A'
pub const FAT_FSINFO_STRUCT_SIGNATURE: [u8; 4] = [0x72, 0x72, 0x41, 0x61]; // 'r','r','A','a'
pub const FAT_FSINFO_TRAIL_SIGNATURE: [u8; 4] = [0x00, 0x00, 0x55, 0xAA]; // 0xAA55_0000 LE
pub const FAT_FSINFO_UNKNOWN: u32 = 0xFFFFFFFF;

pub const FAT_EOD: u8 = 0x00;
pub const FAT_ENTRY_DELETED: u8 = 0xE5;
pub const FAT_MASK: u32 = 0x0FFFFFFF;

pub const FAT_DOT_NAME: &[u8; 11] = b".          ";
pub const FAT_DOTDOT_NAME: &[u8; 11] = b"..         ";
