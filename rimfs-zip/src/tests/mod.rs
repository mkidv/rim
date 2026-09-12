pub mod conformance;
mod reformat;
pub mod spec_compliance;

use super::*;
use rimfs_core::checker::{FsChecker, VerifyReport};
use rimfs_core::formatter::FsFormatter;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::FsTreeResolver;
use rimfs_core::resolver::attr::FileAttributes;
use rimfs_core::resolver::node::FsNode;
use rimfs_core::testing::{
    ExpectedFile, ExpectedLink, assert_dir_entries, assert_dirs, assert_exists, assert_files,
    assert_missing, assert_no_findings, assert_symlinks, basic_tree, file, file_with_attr,
};
use rimio::{MemRimIO, RimRead, RimWrite};

#[test]
fn test_zip_format_fits_1k_buffer() {
    let meta = ZipMeta::default();
    let mut disk_buf = [0u8; 1024];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut formatter = ZipFormatter::new(&mut io, &meta);
    formatter.format(false).unwrap();

    let mut checker = ZipChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&ZipCheckerOptions::default(), &mut report)
        .unwrap();
    assert_no_findings(&report);

    let mut resolver = ZipResolver::new(&mut io, &meta);
    assert!(resolver.exists("/"));
    assert_dir_entries(&mut resolver, "/", &[]);
}

#[test]
fn test_zip_files_dirs_and_symlinks() {
    let meta = ZipMeta::default();
    let mut disk_buf = alloc::vec![0u8; 65536];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut custom_attr = FileAttributes::new_file();
    custom_attr.mode = Some(0o755);
    custom_attr.uid = Some(1000);
    custom_attr.gid = Some(1000);

    let mut tree = basic_tree();
    if let FsNode::Container { children, .. } = &mut tree {
        children[0] = file_with_attr("hello.txt", b"Hello World!", custom_attr);
    }

    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut checker = ZipChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&ZipCheckerOptions::default(), &mut report)
        .unwrap();
    assert_no_findings(&report);

    let mut resolver = ZipResolver::new(&mut io, &meta);
    assert_exists(
        &mut resolver,
        &["hello.txt", "subdir", "subdir/nested.txt", "link_to_hello"],
    );
    assert_files(
        &mut resolver,
        &[
            ExpectedFile {
                path: "hello.txt",
                bytes: b"Hello World!",
            },
            ExpectedFile {
                path: "subdir/nested.txt",
                bytes: b"Nested file content",
            },
        ],
    );
    assert_symlinks(
        &mut resolver,
        &[ExpectedLink {
            path: "link_to_hello",
            target: "hello.txt",
        }],
    );

    let attr = resolver.read_attributes("hello.txt").unwrap();
    assert_eq!(attr.mode, Some(0o100755));
    assert_eq!(attr.uid, Some(1000));
    assert_eq!(attr.gid, Some(1000));

    assert_dir_entries(
        &mut resolver,
        "/",
        &["hello.txt", "subdir", "link_to_hello"],
    );
}

#[test]
fn test_zip_crc_corruption_detection() {
    let meta = ZipMeta::default();
    let mut disk_buf = alloc::vec![0u8; 65536];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut tree = FsNode::new_container(alloc::vec![file("data.bin", b"Valid Payload Data",)]);

    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut checker = ZipChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&ZipCheckerOptions::default(), &mut report)
        .unwrap();
    assert_no_findings(&report);

    // LFH is 30 bytes + 8 bytes ("data.bin") = 38 bytes offset + extra fields
    let mut byte = [0u8; 1];
    io.read_at(65, &mut byte).unwrap();
    byte[0] ^= 0xFF;
    io.write_at(65, &byte).unwrap();

    let mut checker = ZipChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&ZipCheckerOptions::default(), &mut report)
        .unwrap();
    assert!(report.has_error());
}

#[test]
fn test_zip_rejects_out_of_bounds_central_directory() {
    let meta = ZipMeta::default();
    let mut disk_buf = [0u8; types::END_OF_CENTRAL_DIR_FIXED_SIZE];
    disk_buf[0..4].copy_from_slice(&types::END_OF_CENTRAL_DIR_SIG.to_le_bytes());
    disk_buf[12..16].copy_from_slice(&1u32.to_le_bytes());
    disk_buf[16..20].copy_from_slice(&1u32.to_le_bytes());
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut checker = ZipChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&ZipCheckerOptions::default(), &mut report)
        .unwrap();

    assert!(report.has_error());
}

#[test]
fn test_zip_utf8_and_nested_traversal() {
    let meta = ZipMeta::default();
    let mut disk_buf = alloc::vec![0u8; 65536];
    let mut io = MemRimIO::new(&mut disk_buf);

    let mut tree = FsNode::new_container(alloc::vec![
        FsNode::new_dir("documents/français"),
        file(
            "documents/français/rapport_été.txt",
            b"Donn\xc3\xa9es d'\xc3\xa9t\xc3\xa9",
        ),
    ]);

    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut checker = ZipChecker::new(&mut io, &meta);
    let mut report = VerifyReport::default();
    checker
        .check_content(&ZipCheckerOptions::default(), &mut report)
        .unwrap();
    assert_no_findings(&report);

    let mut resolver = ZipResolver::new(&mut io, &meta);
    assert_exists(
        &mut resolver,
        &[
            "documents/français/rapport_été.txt",
            "documents/français",
            "documents",
        ],
    );
    assert_missing(&mut resolver, &["documents/non_existent.txt"]);
    assert_files(
        &mut resolver,
        &[ExpectedFile {
            path: "documents/français/rapport_été.txt",
            bytes: b"Donn\xc3\xa9es d'\xc3\xa9t\xc3\xa9",
        }],
    );
    assert_dirs(&mut resolver, &["documents/français"]);
    assert_dir_entries(&mut resolver, "documents", &["français"]);
}

#[test]
fn test_zip_fallible_try_new_and_deflate_symlink() {
    let meta = ZipMeta::default();
    let mut empty_buf = [0u8; 100];
    let mut io = MemRimIO::new(&mut empty_buf);

    // try_new should fail on empty or unformatted buffer
    let res = ZipResolver::try_new(&mut io, &meta);
    assert!(res.is_err());

    // Now test roundtrip of symlink with deflate compression
    let mut disk_buf = alloc::vec![0u8; 65536];
    let mut io = MemRimIO::new(&mut disk_buf);
    let mut tree = FsNode::new_container(alloc::vec![
        file("target_file.txt", b"Target File Content"),
        FsNode::Symlink {
            name: "link_deflate".into(),
            target: "target_file.txt".into(),
            attr: rimfs_core::resolver::attr::FileAttributes::new_symlink(),
        }
    ]);
    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut resolver = ZipResolver::try_new(&mut io, &meta).unwrap();
    let link_target = resolver.read_link("link_deflate").unwrap();
    assert_eq!(link_target, "target_file.txt");
}
