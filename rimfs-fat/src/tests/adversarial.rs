// SPDX-License-Identifier: MIT

//! Adversarial integrity tripwires testing `FatChecker` against deliberate corruptions.

use alloc::vec::Vec;
use crate::checker::FatChecker;
use crate::formatter::FatFormatter;
use crate::meta::FatMeta;
use rimfs_core::checker::FsChecker;
use rimfs_core::formatter::FsFormatter;
use rimfs_core::testing::{assert_has_error, corrupt_bytes_at};
use rimio::MemRimIO;

fn setup_clean_fat32() -> (FatMeta, Vec<u8>) {
    let meta = FatMeta::new_fat32(32 * 1024 * 1024, Some("FAT_ADV")).expect("FatMeta creation failed");
    let mut disk = vec![0u8; 32 * 1024 * 1024];
    {
        let mut io = MemRimIO::new(&mut disk);
        FatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("format failed");
    }
    (meta, disk)
}

#[test]
fn test_fat_tripwire_corrupted_boot_signature() {
    let (meta, mut disk) = setup_clean_fat32();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt 0xAA55 signature at offset 510
    corrupt_bytes_at(&mut io, 510, &[0x00, 0x00]);

    let mut checker = FatChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "FatChecker must detect corrupted boot signature");
    assert_has_error(&report, "VBR.INVALID");
}

#[test]
fn test_fat_tripwire_fat_copies_mismatch() {
    let (meta, mut disk) = setup_clean_fat32();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt a byte in the second FAT table copy
    if meta.num_fats > 1 {
        let fat_size_bytes = meta.fat_size_sectors as u64 * meta.bytes_per_sector as u64;
        let fat2_offset = meta.fat_offset_bytes + fat_size_bytes;
        corrupt_bytes_at(&mut io, fat2_offset + 8, &[0xDE, 0xAD, 0xBE, 0xEF]);

        let mut checker = FatChecker::new(&mut io, &meta);
        let report = checker.check_all().expect("checker run failed");
        assert!(report.has_error(), "FatChecker must detect mismatch between FAT copies");
        assert_has_error(&report, "FAT.MIRROR");
    }
}
