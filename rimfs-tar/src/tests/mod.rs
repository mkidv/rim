pub mod conformance;
pub mod spec_compliance;

use super::*;
#[cfg(not(feature = "std"))]
use alloc::format;
use rimfs_core::checker::{FsChecker, VerifyReport};
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::FsTreeResolver;
use rimfs_core::testing::{assert_has_error, assert_no_findings, basic_tree};
use rimio::SliceRimIO;
use rimio::{MemRimIO, RimRead, RimWrite};

fn archive_header(kind: u8, size: u64) -> [u8; 512] {
    use zerocopy::IntoBytes;
    let mut header = types::UstarHeader {
        typeflag: kind,
        ..Default::default()
    };
    header.name[0] = b'x';
    types::format_octal(&mut header.size, size);
    let checksum = header.calculate_checksum();
    types::format_octal(&mut header.checksum, checksum as u64);
    header.as_bytes().try_into().unwrap()
}

#[test]
fn checker_and_resolver_reject_incomplete_archives() {
    let meta = TarMeta::default();
    let mut bad_trailer = alloc::vec![0u8; 1024];
    bad_trailer[512] = 1;
    let truncated_payload = archive_header(b'0', 4096).to_vec();
    let mut orphan_name = alloc::vec![0u8; 2048];
    orphan_name[..512].copy_from_slice(&archive_header(b'L', 2));
    orphan_name[512] = b'x';
    for mut bytes in [
        alloc::vec![0; 511],
        alloc::vec![0; 512],
        bad_trailer,
        truncated_payload,
        orphan_name,
    ] {
        let mut io = MemRimIO::new(&mut bytes);
        let mut report = VerifyReport::default();
        let result = TarChecker::new(&mut io, &meta).check_content(&TarCheckerOptions, &mut report);
        assert!(result.is_err() || report.has_error());
        assert!(TarResolver::new(&mut io, &meta).resolve_entry("x").is_err());
    }
}

#[test]
fn checker_and_resolver_share_gnu_long_link_handling() {
    let meta = TarMeta::default();
    let mut bytes = [0; 4096];
    let mut io = MemRimIO::new(&mut bytes);
    let target = "target/".repeat(30);
    let mut injector = TarInjector::new(&mut io, &meta).unwrap();
    injector
        .write_symlink(
            "link",
            &target,
            &rimfs_core::resolver::attr::FileAttributes::new_file(),
        )
        .unwrap();
    injector.flush().unwrap();
    let mut report = VerifyReport::default();
    TarChecker::new(&mut io, &meta)
        .check_content(&TarCheckerOptions, &mut report)
        .unwrap();
    assert_no_findings(&report);
    let (entry, _) = TarResolver::new(&mut io, &meta)
        .resolve_entry("link")
        .unwrap();
    assert_eq!(entry.link_name, target);
}

// Direct creation on zeroed storage and read-only I/O are distinct from conformance.
#[test]
fn test_tar_create_without_formatter_and_read_only_resolver() {
    let meta = TarMeta::default();
    let mut disk_buf = [0u8; 10240];
    let mut io = MemRimIO::new(&mut disk_buf);
    TarInjector::new(&mut io, &meta)
        .unwrap()
        .inject_tree(&mut basic_tree())
        .unwrap();
    let mut slice_io = SliceRimIO::new(&disk_buf);
    let mut resolver = TarResolver::new(&mut slice_io, &meta);
    assert_eq!(
        resolver.read_file("subdir/nested.txt").unwrap(),
        b"Nested file content"
    );
}

#[test]
fn test_tar_header_corruption_detection() {
    let meta = TarMeta::default();
    let mut disk_buf = [0u8; 10240];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut tree = basic_tree();
    let mut injector = TarInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut byte = [0u8; 1];
    io.read_at(0, &mut byte).unwrap();
    byte[0] ^= 0xFF;
    io.write_at(0, &byte).unwrap();

    let mut checker = TarChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&TarCheckerOptions, &mut report)
        .unwrap();
    assert_has_error(&report, "TAR.CHECKSUM");
}

#[test]
fn test_tar_long_path_ustar_and_gnu() {
    let meta = TarMeta::default();
    let mut disk_buf = [0u8; 32768];
    let mut io = MemRimIO::new(&mut disk_buf);

    // 1. Long path that splits cleanly into USTAR prefix + name (> 100 bytes total)
    // prefix: "a".repeat(60) (<= 155), name: "b".repeat(60) (<= 100) -> total 121 chars
    let ustar_path = format!("{}/{}", "a".repeat(60), "b".repeat(60));

    // 2. Single-component long name without slashes (> 100 bytes, e.g. 130 chars) -> forces GNU @LongLink
    let gnu_path = "c".repeat(130);

    let mut injector = TarInjector::new(&mut io, &meta).unwrap();
    let file_attrs = rimfs_core::resolver::attr::FileAttributes::new_file();
    let mut ustar_reader = SliceRimIO::new(b"ustar_content");
    injector
        .write_file(&ustar_path, &mut ustar_reader, 13, &file_attrs)
        .unwrap();
    let mut gnu_reader = SliceRimIO::new(b"gnu_content");
    injector
        .write_file(&gnu_path, &mut gnu_reader, 11, &file_attrs)
        .unwrap();
    injector.flush().unwrap();

    let mut slice_io = SliceRimIO::new(&disk_buf);
    let mut resolver = TarResolver::new(&mut slice_io, &meta);

    let (entry1, _) = resolver.resolve_entry(&ustar_path).unwrap();
    assert_eq!(entry1.name, ustar_path);
    let start1 = entry1.data_offset as usize;
    let end1 = start1 + entry1.size as usize;
    assert_eq!(&disk_buf[start1..end1], b"ustar_content");

    let (entry2, _) = resolver.resolve_entry(&gnu_path).unwrap();
    assert_eq!(entry2.name, gnu_path);
    let start2 = entry2.data_offset as usize;
    let end2 = start2 + entry2.size as usize;
    assert_eq!(&disk_buf[start2..end2], b"gnu_content");
}
