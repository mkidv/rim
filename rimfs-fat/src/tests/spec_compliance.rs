// SPDX-License-Identifier: MIT

//! FAT on-disk specification compliance tests (offsets, packing, canonical values).

use crate::constant::*;
use crate::types::{Fat12_16Ebpb, Fat32Ebpb, FatCommonBpb, FatEntry, FatLFNEntry, FatVbr};
use zerocopy::IntoBytes;

// Compile-time layout guarantees
const _: () = {
    assert!(core::mem::size_of::<FatCommonBpb>() == 36);
    assert!(core::mem::size_of::<Fat12_16Ebpb>() == 26);
    assert!(core::mem::size_of::<Fat32Ebpb>() == 54);
    assert!(core::mem::size_of::<FatVbr>() == 512);
    assert!(core::mem::size_of::<FatEntry>() == 32);
    assert!(core::mem::size_of::<FatLFNEntry>() == 32);
};

#[test]
fn test_fat_bpb_wire_format_offsets() {
    let meta = crate::meta::FatMeta::new_fat32(32 * 1024 * 1024, Some("FAT32_TEST")).unwrap();
    let vbr = FatVbr::from_meta(&meta);
    let bytes = vbr.as_bytes();

    assert_eq!(&bytes[0..3], &FAT_JUMP_BOOT, "Jump boot at 0x00");
    assert_eq!(&bytes[11..13], &512u16.to_le_bytes(), "Bytes per sector at 0x0B");
    assert_eq!(bytes[13], meta.sectors_per_cluster as u8, "Sectors per cluster at 0x0D");
    assert_eq!(bytes[16], meta.num_fats, "Number of FATs at 0x10");
    assert_eq!(bytes[21], FAT_MEDIA_DESCRIPTOR, "Media descriptor at 0x15");
    assert_eq!(&bytes[510..512], &[0x55, 0xAA], "End marker at 0x1FE");
}

#[test]
fn test_fat_attribute_canonical_constants() {
    use crate::attr::FatFileAttributes;
    assert_eq!(FatFileAttributes::READ_ONLY.bits(), 0x01);
    assert_eq!(FatFileAttributes::HIDDEN.bits(), 0x02);
    assert_eq!(FatFileAttributes::SYSTEM.bits(), 0x04);
    assert_eq!(FatFileAttributes::VOLUME_ID.bits(), 0x08);
    assert_eq!(FatFileAttributes::DIRECTORY.bits(), 0x10);
    assert_eq!(FatFileAttributes::ARCHIVE.bits(), 0x20);
    assert_eq!(FatFileAttributes::LFN.bits(), 0x0F);
}

#[test]
fn test_fat_dir_entry_wire_format() {
    let entry = FatEntry::new(
        *b"TESTFILETXT",
        0x20, // ARCHIVE
        2,    // first cluster
        100,  // file size
        0x5021, // date
        0x6000, // time
        0,
    );
    let bytes = entry.as_bytes();
    assert_eq!(bytes.len(), 32);
    assert_eq!(&bytes[0..11], b"TESTFILETXT", "Short name at 0x00");
    assert_eq!(bytes[11], 0x20, "Attribute byte at 0x0B");
    assert_eq!(&bytes[28..32], &100u32.to_le_bytes(), "File size at 0x1C");
}
