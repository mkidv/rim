// SPDX-License-Identifier: MIT

//! Adversarial integrity tripwires testing `ExFatChecker` against deliberate corruptions.

use alloc::vec::Vec;
use crate::checker::ExFatChecker;
use crate::formatter::ExFatFormatter;
use crate::meta::ExFatMeta;
use rimfs_core::checker::FsChecker;
use rimfs_core::formatter::FsFormatter;
use rimfs_core::meta::FsMeta;
use rimfs_core::testing::{assert_has_error, corrupt_byte_at};
use rimio::MemRimIO;

fn setup_clean_exfat() -> (ExFatMeta, Vec<u8>) {
    let meta = ExFatMeta::new(32 * 1024 * 1024, Some("EXFAT_ADV")).expect("ExFatMeta creation failed");
    let mut disk = vec![0u8; 32 * 1024 * 1024];
    {
        let mut io = MemRimIO::new(&mut disk);
        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("format failed");
    }
    (meta, disk)
}

#[test]
fn test_exfat_tripwire_corrupted_boot_checksum() {
    let (meta, mut disk) = setup_clean_exfat();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt a byte in the main VBR (e.g. at offset 4)
    corrupt_byte_at(&mut io, 4, |b| b ^ 0xFF);

    let mut checker = ExFatChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "ExFatChecker must detect VBR checksum mismatch");
    assert_has_error(&report, "VBR.CHK");
}

#[test]
fn test_exfat_tripwire_orphan_cluster() {
    let (meta, mut disk) = setup_clean_exfat();
    let mut io = MemRimIO::new(&mut disk);

    // Set an unused cluster bit in the allocation bitmap
    let bitmap_offset = meta.unit_offset(meta.bitmap_cluster);
    // Mutate byte at bitmap_offset + 10 to 0xFF (marking 8 unallocated clusters as used)
    corrupt_byte_at(&mut io, bitmap_offset + 10, |_| 0xFF);

    let mut checker = ExFatChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "ExFatChecker must detect orphan clusters / bitmap mismatch");
    assert!(
        report.findings.iter().any(|f| f.code == "WALK.ORPHAN" || f.code == "XREF.BITMAPFAT"),
        "expected WALK.ORPHAN or XREF.BITMAPFAT, got {:?}",
        report.findings
    );
}
