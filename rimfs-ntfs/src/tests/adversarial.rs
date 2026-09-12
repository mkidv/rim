// SPDX-License-Identifier: MIT

//! Adversarial integrity tripwires testing `NtfsChecker` against deliberate corruptions.

use alloc::vec::Vec;
use crate::checker::NtfsChecker;
use crate::constant::*;
use crate::formatter::NtfsFormatter;
use crate::meta::NtfsMeta;
use crate::types::*;
use crate::view::attr_view::AttrView;
use rimfs_core::checker::{FsChecker, Severity};
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::testing::{assert_has_error, corrupt_byte_at, corrupt_bytes_at};
use rimio::prelude::*;

fn setup_clean_volume() -> (NtfsMeta, Vec<u8>) {
    let meta = NtfsMeta::new(5 * 1024 * 1024, Some("NTFS_ADV")).expect("NtfsMeta creation failed");
    let mut disk = vec![0u8; 5 * 1024 * 1024];
    {
        let mut io = MemRimIO::new(&mut disk);
        NtfsFormatter::new(&mut io, &meta)
            .format(true)
            .expect("format failed");
    }
    (meta, disk)
}

#[test]
fn test_ntfs_tripwire_corrupted_boot_oem() {
    let (meta, mut disk) = setup_clean_volume();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt OEM ID from "NTFS    " to "XXXX    "
    corrupt_bytes_at(&mut io, 3, b"XXXX");

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "NtfsChecker must detect corrupted boot OEM ID");
    assert_has_error(&report, "BOOT.OEM");
}

#[test]
fn test_ntfs_tripwire_corrupted_boot_signature() {
    let (meta, mut disk) = setup_clean_volume();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt end marker 0xAA55 at offset 510
    corrupt_bytes_at(&mut io, 510, &[0x00, 0x00]);

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "NtfsChecker must detect corrupted boot signature");
    assert_has_error(&report, "BOOT.SIG");
}

#[test]
fn test_ntfs_tripwire_boot_mirror_mismatch() {
    let (meta, mut disk) = setup_clean_volume();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt byte in backup boot sector
    let backup_offset = meta.backup_boot_sector_offset();
    corrupt_byte_at(&mut io, backup_offset + 3, |b| b ^ 0xFF);

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "NtfsChecker must detect primary/alternate boot mismatch");
    assert_has_error(&report, "BOOT.MIRROR");
}

#[test]
fn test_ntfs_tripwire_mft_record_signature() {
    let (meta, mut disk) = setup_clean_volume();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt magic of MFT record 0 ($MFT) from "FILE" to "BAAD"
    let mft_offset = meta.mft_record_offset(MFT_RECORD_MFT);
    corrupt_bytes_at(&mut io, mft_offset, b"BAAD");

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "NtfsChecker must detect invalid MFT record magic");
    assert_has_error(&report, "MFT.SIG");
}

#[test]
fn test_ntfs_tripwire_mft_trailer_corruption() {
    let (meta, mut disk) = setup_clean_volume();
    let mut io = MemRimIO::new(&mut disk);

    // Corrupt USA trailer at end of first sector of MFT record 0 (offset 510 in record)
    let mft_offset = meta.mft_record_offset(MFT_RECORD_MFT);
    let trailer_offset = mft_offset + 510;
    corrupt_byte_at(&mut io, trailer_offset, |b| b ^ 0xFF);

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().expect("checker run failed");
    assert!(report.has_error(), "NtfsChecker must detect USA fixup trailer corruption");
    assert_has_error(&report, "MFT.TRAILER");
}

#[test]
fn test_ntfs_tripwire_mft_usa_bounds() {
    let (meta, mut disk) = setup_clean_volume();
    let mut io = MemRimIO::new(&mut disk);

    let mft_rec0_offset = meta.mft_record_offset(0);

    // 1. Corrupt usa_count to 1 (invalid for 1024-byte record with 512-byte sectors)
    corrupt_bytes_at(&mut io, mft_rec0_offset + 6, &1u16.to_le_bytes());
    let mut checker = NtfsChecker::new(&mut io, &meta);
    let rep = checker.check_all().unwrap();
    assert!(rep.has_error(), "Must detect invalid usa_count");
    assert_has_error(&rep, "MFT.USA");

    // 2. Corrupt usa_offset to an out-of-bounds offset (2000)
    corrupt_bytes_at(&mut io, mft_rec0_offset + 4, &2000u16.to_le_bytes());
    let mut checker2 = NtfsChecker::new(&mut io, &meta);
    let rep2 = checker2.check_all().unwrap();
    assert!(rep2.has_error(), "Must detect out-of-bounds usa_offset");
    assert_has_error(&rep2, "MFT.USA");
}

#[test]
fn test_ntfs_tripwire_index_corruption() {
    use crate::core::traits::FsNode;
    use crate::injector::NtfsInjector;
    use rimfs_core::resolver::FileAttributes;
    use crate::upcase::UpcaseFlavor;

    let meta = NtfsMeta::new_custom(
        20 * 1024 * 1024,
        Some("TRIPWIRE"),
        None,
        512,
        512,
        1024,
        4096,
        0,
        UpcaseFlavor::Legacy,
    )
    .unwrap();

    let mut buffer = vec![0u8; 20 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut buffer);

    NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

    let mut children = Vec::new();
    for i in 0..80 {
        children.push(FsNode::new_file(
            format!("file_{:03}.dat", i),
            vec![0x12; 60],
        ));
    }
    let mut tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children,
    };

    let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    // 1. Corrupt a child VCN in $INDEX_ROOT on disk:
    let root_offset = meta.lcn_to_offset(meta.mft_lcn) + 5 * meta.mft_record_size as u64;
    let mut root_record_bytes = vec![0u8; meta.mft_record_size as usize];
    io.read_at(root_offset, &mut root_record_bytes).unwrap();

    crate::utils::decode_usa_fixup(&mut root_record_bytes, meta.bytes_per_sector as usize);

    let view = crate::view::mft_view::MftRecordView::new(&root_record_bytes).unwrap();
    let root_attr = view
        .find_named(NtfsAttributeType::IndexRoot, Some("$I30"))
        .unwrap()
        .unwrap();
    let (val_offset, val_len) = if let Ok(AttrView::Resident { value, .. }) = root_attr.as_view() {
        let off = (value.as_ptr() as usize) - (root_record_bytes.as_ptr() as usize);
        (off, value.len())
    } else {
        panic!("Expected resident index root");
    };

    let bad_vcn = 999u64;
    let mut corrupted_record_bytes = root_record_bytes.clone();
    let vcn_pos = val_offset + val_len - 8;
    corrupted_record_bytes[vcn_pos..vcn_pos + 8].copy_from_slice(&bad_vcn.to_le_bytes());

    crate::utils::apply_usa_fixup(&mut corrupted_record_bytes, meta.bytes_per_sector as usize);
    io.write_at(root_offset, &corrupted_record_bytes).unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let bad_rep = checker.check_all().unwrap();
    assert!(bad_rep.has_error(), "NtfsChecker must detect corrupted child VCN");
    assert!(
        bad_rep
            .findings
            .iter()
            .any(|f| f.code == "IDX.VCN" && f.sev == Severity::Error)
    );

    // 2. Corrupt $INDEX_ROOT by shortening LAST_ENTRY's entry_length so trailing bytes appear
    let mut corrupted_trailing_bytes = root_record_bytes.clone();
    let last_entry_offset = val_offset + val_len - 24;
    let new_entry_len = 16u16;
    corrupted_trailing_bytes[last_entry_offset + 8..last_entry_offset + 10]
        .copy_from_slice(&new_entry_len.to_le_bytes());
    crate::utils::apply_usa_fixup(
        &mut corrupted_trailing_bytes,
        meta.bytes_per_sector as usize,
    );
    io.write_at(root_offset, &corrupted_trailing_bytes).unwrap();

    let mut checker2 = NtfsChecker::new(&mut io, &meta);
    let bad_rep2 = checker2.check_all().unwrap();
    assert!(bad_rep2.has_error(), "NtfsChecker must detect trailing entries or mismatched length");
    assert!(
        bad_rep2
            .findings
            .iter()
            .any(|f| f.code == "IDX.ROOT" && f.sev == Severity::Error)
    );
}
