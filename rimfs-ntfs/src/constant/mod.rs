// SPDX-License-Identifier: MIT
//! NTFS Constants
//!
//! Reference: Microsoft NTFS On-Disk Format (public documentation)
//! and Linux ntfs-3g sources.

/// Boot sector signature
pub const NTFS_BOOT_SIGNATURE: [u8; 8] = *b"NTFS    ";

/// Sector size (standard)
pub const NTFS_SECTOR_SIZE: u16 = 512;

/// MFT record signature
pub const NTFS_FILE_SIGNATURE: [u8; 4] = *b"FILE";

/// Index record signature
pub const NTFS_INDX_SIGNATURE: [u8; 4] = *b"INDX";

/// Standard MFT record size
pub const NTFS_MFT_RECORD_SIZE: u32 = 1024;

/// Standard index record size (4096 bytes)
pub const NTFS_INDEX_RECORD_SIZE: u32 = 4096;

/// Default cluster size (4 KB)
pub const NTFS_DEFAULT_CLUSTER_SIZE: u32 = 4096;

// System file MFT record numbers
pub const MFT_RECORD_MFT: u64 = 0; // $MFT
pub const MFT_RECORD_MFTMIRR: u64 = 1; // $MFTMirr
pub const MFT_RECORD_LOGFILE: u64 = 2; // $LogFile
pub const MFT_RECORD_VOLUME: u64 = 3; // $Volume
pub const MFT_RECORD_ATTRDEF: u64 = 4; // $AttrDef
pub const MFT_RECORD_ROOT: u64 = 5; // . (root directory)
pub const MFT_RECORD_BITMAP: u64 = 6; // $Bitmap
pub const MFT_RECORD_BOOT: u64 = 7; // $Boot
pub const MFT_RECORD_BADCLUS: u64 = 8; // $BadClus
pub const MFT_RECORD_SECURE: u64 = 9; // $Secure
pub const MFT_RECORD_UPCASE: u64 = 10; // $UpCase
pub const MFT_RECORD_EXTEND: u64 = 11; // $Extend

// Reserved system records (12-15 are in-use but empty, 16-23 are free)
pub const MFT_RECORD_RESERVED_START: u64 = 12;
pub const MFT_RECORD_FREE_START: u64 = 16;
pub const MFT_RECORD_USER_START: u64 = 24;

// $Extend children (assigned to > 24 according to specs)
pub const MFT_RECORD_QUOTA: u64 = 24; // $Extend\$Quota
pub const MFT_RECORD_OBJID: u64 = 25; // $Extend\$ObjId
pub const MFT_RECORD_REPARSE: u64 = 26; // $Extend\$Reparse
pub const MFT_RECORD_USNJRNL: u64 = 27; // $Extend\$UsnJrnl
pub const MFT_RECORD_FIRST_USER: u64 = 1024; // First user file/directory

/// Minimum reserved MFT records for system files
pub const NTFS_RESERVED_MFT_RECORDS: u64 = 1024;

// Attribute types
pub const ATTR_STANDARD_INFORMATION: u32 = 0x10;
pub const ATTR_ATTRIBUTE_LIST: u32 = 0x20;
pub const ATTR_FILE_NAME: u32 = 0x30;
pub const ATTR_OBJECT_ID: u32 = 0x40;
pub const ATTR_SECURITY_DESCRIPTOR: u32 = 0x50;
pub const ATTR_VOLUME_NAME: u32 = 0x60;
pub const ATTR_VOLUME_INFORMATION: u32 = 0x70;
pub const ATTR_DATA: u32 = 0x80;
pub const ATTR_INDEX_ROOT: u32 = 0x90;
pub const ATTR_INDEX_ALLOCATION: u32 = 0xA0;
pub const ATTR_BITMAP: u32 = 0xB0;
pub const ATTR_REPARSE_POINT: u32 = 0xC0;
pub const ATTR_EA_INFORMATION: u32 = 0xD0;
pub const ATTR_EA: u32 = 0xE0;
pub const ATTR_LOGGED_UTILITY_STREAM: u32 = 0x100;
pub const ATTR_END: u32 = 0xFFFFFFFF;

// Security IDs
pub const SECURITY_ID_EVERYONE: u32 = 0x100;
pub const SECURITY_ID_SYSTEM: u32 = 0x101;

#[allow(unused_imports)]
pub use crate::types::sid::*;

// Filename namespace
pub const FILE_NAME_POSIX: u8 = 0;
pub const FILE_NAME_WIN32: u8 = 1;
pub const FILE_NAME_DOS: u8 = 2;
pub const FILE_NAME_WIN32_AND_DOS: u8 = 3;

/// End-of-cluster marker (not applicable for NTFS in the same way as FAT,
/// but we use a sentinel for allocation tracking)
pub const NTFS_CLUSTER_UNUSED: u64 = 0;

pub mod upcase;
