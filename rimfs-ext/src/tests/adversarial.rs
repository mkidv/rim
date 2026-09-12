// SPDX-License-Identifier: MIT

//! Adversarial integrity tripwires testing `ExtChecker` against deliberate corruptions.

use alloc::vec::Vec;
use crate::checker::ExtChecker;
use crate::constant::*;
use crate::formatter::ExtFormatter;
use crate::meta::ExtMeta;
use rimfs_core::checker::FsChecker;
use rimfs_core::formatter::FsFormatter;
use rimfs_core::testing::{assert_has_error, corrupt_byte_at, corrupt_bytes_at};
use rimio::MemRimIO;

fn setup_clean_ext4() -> (ExtMeta, Vec<u8>) {
    let meta = ExtMeta::new(32 * 1024 * 1024, Some("EXT_ADV")).expect("ExtMeta creation failed");
    let mut disk = vec![0u8; 32 * 1024 * 1024];
    {
        let mut io = MemRimIO::new(&mut disk);
        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("format failed");
    }
    (meta, disk)
}

#[test]
fn test_ext_tripwire_corrupted_magic() {
    let (meta, mut disk) = setup_clean_ext4();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt magic at EXT_SUPERBLOCK_OFFSET + 0x38
    corrupt_bytes_at(&mut io, EXT_SUPERBLOCK_OFFSET + 0x38, &[0x00, 0x00]);

    let mut checker = ExtChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "ExtChecker must detect invalid superblock magic");
    assert_has_error(&report, "SB.MAGIC");
}

#[test]
fn test_ext_tripwire_corrupted_free_blocks_counter() {
    let (meta, mut disk) = setup_clean_ext4();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt free blocks count in superblock at EXT_SUPERBLOCK_OFFSET + 0x0C
    corrupt_byte_at(&mut io, EXT_SUPERBLOCK_OFFSET + 0x0C, |b| b.wrapping_add(10));

    let mut checker = ExtChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "ExtChecker must detect free blocks mismatch");
    assert_has_error(&report, "SB.FREE_BLOCKS");
}
