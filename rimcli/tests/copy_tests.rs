// SPDX-License-Identifier: MIT

use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use rimcli::copy::options::{
    CopyOptions, MetadataPolicy, OverwritePolicy, UnsupportedMetadataPolicy,
};
use rimcli::copy::report::CopyWarningKind;
use rimcli::copy::{CopyError, copy_tree};

use rimfs::core::formatter::FsFormatter;
use rimfs::core::injector::FsTreeInjector;
use rimfs::core::resolver::FsTreeResolver;
use rimfs::core::resolver::attr::FileAttributes;
use rimfs::core::resolver::node::FsNode;
use rimfs::core::{StdInjector, StdResolver};
use rimfs::{exfat, ext, fat, iso, ntfs, tar, zip};
use rimio::{MemRimIO, VecRimIO};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FsKind {
    Fat,
    ExFat,
    Ext,
    Ntfs,
    Tar,
    Zip,
    Iso,
}

fn fs_image_size(kind: FsKind) -> usize {
    match kind {
        FsKind::Fat => 64 * 1024 * 1024,
        FsKind::ExFat => 64 * 1024 * 1024,
        FsKind::Ext => 64 * 1024 * 1024,
        FsKind::Ntfs => 64 * 1024 * 1024,
        FsKind::Tar => 4 * 1024 * 1024,
        FsKind::Zip => 4 * 1024 * 1024,
        FsKind::Iso => 16 * 1024 * 1024,
    }
}

fn populate_sample_tree() -> FsNode<'static> {
    FsNode::new_container(vec![
        FsNode::new_file_from_source(
            "hello.txt",
            Box::new(VecRimIO::new(b"Hello universal copy engine!\n".to_vec())),
            FileAttributes::new_file(),
        ),
        FsNode::new_file_from_source(
            "empty.bin",
            Box::new(VecRimIO::new(Vec::new())),
            FileAttributes::new_file(),
        ),
        FsNode::Dir {
            name: "sub".to_string(),
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::new_file_from_source(
                    "nested.txt",
                    Box::new(VecRimIO::new(b"Nested payload content".to_vec())),
                    FileAttributes::new_file(),
                ),
                FsNode::Dir {
                    name: "deep".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::new_file_from_source(
                        "data.bin",
                        Box::new(VecRimIO::new(vec![0x42; 8192])),
                        FileAttributes::new_file(),
                    )],
                },
            ],
        },
    ])
}

fn build_populated_fs(kind: FsKind) -> Vec<u8> {
    let mut bytes = vec![0u8; fs_image_size(kind)];
    let mut tree = populate_sample_tree();
    match kind {
        FsKind::Fat => {
            let meta = fat::FatMeta::new_fat32(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            fat::FatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            fat::FatInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        FsKind::ExFat => {
            let meta = exfat::ExFatMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            exfat::ExFatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            exfat::ExFatInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        FsKind::Ext => {
            let meta = ext::ExtMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ext::ExtFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            ext::ExtInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        FsKind::Ntfs => {
            let meta = ntfs::NtfsMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ntfs::NtfsFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            ntfs::NtfsInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        FsKind::Tar => {
            let meta = tar::TarMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            tar::TarFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            tar::TarInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        FsKind::Zip => {
            let meta = zip::ZipMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            zip::ZipFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            zip::ZipInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
        FsKind::Iso => {
            let meta = iso::IsoMeta::new(bytes.len() as u64, Some("SRC")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            iso::IsoFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
            iso::IsoInjector::new(&mut io, &meta)
                .unwrap()
                .inject_tree(&mut tree)
                .unwrap();
        }
    }
    bytes
}

fn build_empty_fs(kind: FsKind) -> Vec<u8> {
    let mut bytes = vec![0u8; fs_image_size(kind)];
    match kind {
        FsKind::Fat => {
            let meta = fat::FatMeta::new_fat32(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            fat::FatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
        FsKind::ExFat => {
            let meta = exfat::ExFatMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            exfat::ExFatFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
        FsKind::Ext => {
            let meta = ext::ExtMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ext::ExtFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
        FsKind::Ntfs => {
            let meta = ntfs::NtfsMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            ntfs::NtfsFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
        FsKind::Tar => {
            let meta = tar::TarMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            tar::TarFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
        FsKind::Zip => {
            let meta = zip::ZipMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            zip::ZipFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
        FsKind::Iso => {
            let meta = iso::IsoMeta::new(bytes.len() as u64, Some("DST")).unwrap();
            let mut io = MemRimIO::new(&mut bytes);
            iso::IsoFormatter::new(&mut io, &meta)
                .format(false)
                .unwrap();
        }
    }
    bytes
}

fn verify_fs_contents(kind: FsKind, bytes: &mut [u8]) {
    let bytes_len = bytes.len() as u64;
    let mut io = MemRimIO::new(bytes);
    match kind {
        FsKind::Fat => {
            let meta = fat::FatMeta::from_io(&mut io).unwrap();
            let mut resolver = fat::FatResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
            assert_eq!(
                resolver.read_file("sub/nested.txt").unwrap(),
                b"Nested payload content"
            );
            assert_eq!(resolver.read_file("sub/deep/data.bin").unwrap().len(), 8192);
        }
        FsKind::ExFat => {
            let meta = exfat::ExFatMeta::from_io(&mut io).unwrap();
            let mut resolver = exfat::ExFatResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
            assert_eq!(
                resolver.read_file("sub/nested.txt").unwrap(),
                b"Nested payload content"
            );
            assert_eq!(resolver.read_file("sub/deep/data.bin").unwrap().len(), 8192);
        }
        FsKind::Ext => {
            let meta = ext::ExtMeta::from_io(&mut io).unwrap();
            let mut resolver = ext::ExtResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
            assert_eq!(
                resolver.read_file("sub/nested.txt").unwrap(),
                b"Nested payload content"
            );
            assert_eq!(resolver.read_file("sub/deep/data.bin").unwrap().len(), 8192);
        }
        FsKind::Ntfs => {
            let meta = ntfs::NtfsMeta::from_io(&mut io).unwrap();
            let mut resolver = ntfs::NtfsResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
            assert_eq!(
                resolver.read_file("sub/nested.txt").unwrap(),
                b"Nested payload content"
            );
            assert_eq!(resolver.read_file("sub/deep/data.bin").unwrap().len(), 8192);
        }
        FsKind::Tar => {
            let meta = tar::TarMeta::new(bytes_len, Some("VERIFY")).unwrap();
            let mut resolver = tar::TarResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
        }
        FsKind::Zip => {
            let meta = zip::ZipMeta::new(bytes_len, Some("VERIFY")).unwrap();
            let mut resolver = zip::ZipResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
        }
        FsKind::Iso => {
            let meta = iso::IsoMeta::new(bytes_len, Some("VERIFY")).unwrap();
            let mut resolver = iso::IsoResolver::new(&mut io, &meta);
            assert_eq!(
                resolver.read_file("hello.txt").unwrap(),
                b"Hello universal copy engine!\n"
            );
        }
    }
}

fn transfer_fs_to_fs(src_kind: FsKind, dst_kind: FsKind) {
    let mut src_bytes = build_populated_fs(src_kind);
    let mut dst_bytes = build_empty_fs(dst_kind);

    let src_len = src_bytes.len() as u64;
    let dst_len = dst_bytes.len() as u64;

    let mut src_io = MemRimIO::new(&mut src_bytes);
    let mut dst_io = MemRimIO::new(&mut dst_bytes);
    let options = CopyOptions::default();

    // Dynamically dispatch source resolver into destination injector
    match src_kind {
        FsKind::Fat => {
            let meta = fat::FatMeta::from_io(&mut src_io).unwrap();
            let mut resolver = fat::FatResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
        FsKind::ExFat => {
            let meta = exfat::ExFatMeta::from_io(&mut src_io).unwrap();
            let mut resolver = exfat::ExFatResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
        FsKind::Ext => {
            let meta = ext::ExtMeta::from_io(&mut src_io).unwrap();
            let mut resolver = ext::ExtResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
        FsKind::Ntfs => {
            let meta = ntfs::NtfsMeta::from_io(&mut src_io).unwrap();
            let mut resolver = ntfs::NtfsResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
        FsKind::Tar => {
            let meta = tar::TarMeta::new(src_len, Some("SRC")).unwrap();
            let mut resolver = tar::TarResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
        FsKind::Zip => {
            let meta = zip::ZipMeta::new(src_len, Some("SRC")).unwrap();
            let mut resolver = zip::ZipResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
        FsKind::Iso => {
            let meta = iso::IsoMeta::new(src_len, Some("SRC")).unwrap();
            let mut resolver = iso::IsoResolver::new(&mut src_io, &meta);
            inject_into_dest(dst_kind, &mut dst_io, dst_len, &mut resolver, &options);
        }
    }

    verify_fs_contents(dst_kind, &mut dst_bytes);
}

fn inject_into_dest(
    dst_kind: FsKind,
    dst_io: &mut MemRimIO<'_>,
    dst_len: u64,
    resolver: &mut dyn FsTreeResolver,
    options: &CopyOptions,
) {
    match dst_kind {
        FsKind::Fat => {
            let meta = fat::FatMeta::from_io(dst_io).unwrap();
            let mut injector = fat::FatInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
        FsKind::ExFat => {
            let meta = exfat::ExFatMeta::from_io(dst_io).unwrap();
            let mut injector = exfat::ExFatInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
        FsKind::Ext => {
            let meta = ext::ExtMeta::from_io(dst_io).unwrap();
            let mut injector = ext::ExtInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
        FsKind::Ntfs => {
            let meta = ntfs::NtfsMeta::from_io(dst_io).unwrap();
            let mut injector = ntfs::NtfsInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
        FsKind::Tar => {
            let meta = tar::TarMeta::new(dst_len, Some("DST")).unwrap();
            let mut injector = tar::TarInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
        FsKind::Zip => {
            let meta = zip::ZipMeta::new(dst_len, Some("DST")).unwrap();
            let mut injector = zip::ZipInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
        FsKind::Iso => {
            let meta = iso::IsoMeta::new(dst_len, Some("DST")).unwrap();
            let mut injector = iso::IsoInjector::new(dst_io, &meta).unwrap();
            copy_tree(resolver, &mut injector, "/*", options, None).unwrap();
        }
    }
}

// -----------------------------------------------------------------------------
// Cross-filesystem matrix tests
// -----------------------------------------------------------------------------

#[test]
fn test_copy_fat32_to_ext4() {
    transfer_fs_to_fs(FsKind::Fat, FsKind::Ext);
}

#[test]
fn test_copy_ext4_to_fat32() {
    transfer_fs_to_fs(FsKind::Ext, FsKind::Fat);
}

#[test]
fn test_copy_ext4_to_ntfs() {
    transfer_fs_to_fs(FsKind::Ext, FsKind::Ntfs);
}

#[test]
fn test_copy_ntfs_to_ext4() {
    transfer_fs_to_fs(FsKind::Ntfs, FsKind::Ext);
}

#[test]
fn test_copy_exfat_to_ntfs() {
    transfer_fs_to_fs(FsKind::ExFat, FsKind::Ntfs);
}

#[test]
fn test_copy_tar_to_fat32() {
    transfer_fs_to_fs(FsKind::Tar, FsKind::Fat);
}

#[test]
fn test_copy_zip_to_ext4() {
    transfer_fs_to_fs(FsKind::Zip, FsKind::Ext);
}

#[test]
fn test_copy_iso_to_ext4() {
    transfer_fs_to_fs(FsKind::Iso, FsKind::Ext);
}

// -----------------------------------------------------------------------------
// Host filesystem tests
// -----------------------------------------------------------------------------

#[test]
fn test_copy_host_to_host() {
    let temp_root = std::env::temp_dir().join(format!("rim_copy_h2h_{}", std::process::id()));
    let src_dir = temp_root.join("src");
    let dst_dir = temp_root.join("dst");

    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(src_dir.join("sub/deep")).unwrap();

    fs::write(src_dir.join("hello.txt"), b"Hello from host!").unwrap();
    fs::write(src_dir.join("sub/deep/test.bin"), vec![0xAB; 4096]).unwrap();

    let mut resolver = StdResolver::new();
    let mut injector = StdInjector::new(&dst_dir).unwrap();
    let options = CopyOptions::default();

    let src_str = format!("{}/*", src_dir.to_str().unwrap());
    let report = copy_tree(&mut resolver, &mut injector, &src_str, &options, None).unwrap();

    assert_eq!(report.files_copied, 2);
    assert_eq!(report.directories_created, 2);
    assert_eq!(report.bytes_transferred, 16 + 4096);

    assert_eq!(
        fs::read(dst_dir.join("hello.txt")).unwrap(),
        b"Hello from host!"
    );
    assert_eq!(
        fs::read(dst_dir.join("sub/deep/test.bin")).unwrap(),
        vec![0xAB; 4096]
    );

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn test_copy_host_to_ext4_and_back_to_host() {
    let temp_root = std::env::temp_dir().join(format!("rim_copy_h2e2h_{}", std::process::id()));
    let src_dir = temp_root.join("src");
    let back_dir = temp_root.join("back");

    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(src_dir.join("nested")).unwrap();

    let payload = b"Streaming host to EXT4 and roundtrip back to host!";
    fs::write(src_dir.join("nested/payload.txt"), payload).unwrap();

    // 1. Host -> EXT4 image
    let mut ext_bytes = build_empty_fs(FsKind::Ext);
    let mut ext_io = MemRimIO::new(&mut ext_bytes);
    let ext_meta = ext::ExtMeta::from_io(&mut ext_io).unwrap();
    let mut ext_injector = ext::ExtInjector::new(&mut ext_io, &ext_meta).unwrap();

    let mut host_resolver = StdResolver::new();
    let src_str = format!("{}/*", src_dir.to_str().unwrap());
    copy_tree(
        &mut host_resolver,
        &mut ext_injector,
        &src_str,
        &CopyOptions::default(),
        None,
    )
    .unwrap();

    // 2. EXT4 image -> Host
    let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);
    let mut back_injector = StdInjector::new(&back_dir).unwrap();
    copy_tree(
        &mut ext_resolver,
        &mut back_injector,
        "/*",
        &CopyOptions::default(),
        None,
    )
    .unwrap();

    assert_eq!(
        fs::read(back_dir.join("nested/payload.txt")).unwrap(),
        payload
    );

    let _ = fs::remove_dir_all(&temp_root);
}

// -----------------------------------------------------------------------------
// Policies and edge case tests
// -----------------------------------------------------------------------------

#[test]
fn test_unsupported_symlink_policies() {
    let mut link_tree = FsNode::new_container(vec![
        FsNode::new_file("real.txt", b"real target".to_vec()),
        FsNode::Symlink {
            name: "link.lnk".to_string(),
            target: "real.txt".to_string(),
            attr: FileAttributes::new_symlink(),
        },
    ]);

    // Build EXT4 with a symlink
    let mut ext_bytes = vec![0u8; fs_image_size(FsKind::Ext)];
    let ext_meta = ext::ExtMeta::new(ext_bytes.len() as u64, Some("SYM")).unwrap();
    let mut ext_io = MemRimIO::new(&mut ext_bytes);
    ext::ExtFormatter::new(&mut ext_io, &ext_meta)
        .format(false)
        .unwrap();
    ext::ExtInjector::new(&mut ext_io, &ext_meta)
        .unwrap()
        .inject_tree(&mut link_tree)
        .unwrap();

    // Policy 1: Error on FAT (FAT doesn't support symlinks)
    {
        let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);
        let mut fat_bytes = build_empty_fs(FsKind::Fat);
        let mut fat_io = MemRimIO::new(&mut fat_bytes);
        let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
        let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

        let options = CopyOptions {
            unsupported_policy: UnsupportedMetadataPolicy::Error,
            ..Default::default()
        };
        let res = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None);
        assert!(matches!(res, Err(CopyError::UnsupportedFeature { .. })));
    }

    // Policy 2: Warn on FAT
    {
        let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);
        let mut fat_bytes = build_empty_fs(FsKind::Fat);
        let mut fat_io = MemRimIO::new(&mut fat_bytes);
        let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
        let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

        let options = CopyOptions {
            unsupported_policy: UnsupportedMetadataPolicy::Warn,
            ..Default::default()
        };
        let report = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None).unwrap();
        assert_eq!(report.files_copied, 1);
        assert_eq!(report.symlinks_created, 0);
        assert_eq!(report.warnings.len(), 1);
        assert!(matches!(
            report.warnings[0].kind,
            CopyWarningKind::UnsupportedSymlink { .. }
        ));
    }

    // Policy 3: Ignore on FAT
    {
        let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);
        let mut fat_bytes = build_empty_fs(FsKind::Fat);
        let mut fat_io = MemRimIO::new(&mut fat_bytes);
        let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
        let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

        let options = CopyOptions {
            unsupported_policy: UnsupportedMetadataPolicy::Ignore,
            ..Default::default()
        };
        let report = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None).unwrap();
        assert_eq!(report.files_copied, 1);
        assert_eq!(report.symlinks_created, 0);
        assert_eq!(report.warnings.len(), 0);
    }
}

#[test]
fn test_case_sensitive_destination_accepts_case_variants() {
    let mut tree = FsNode::new_container(vec![
        FsNode::new_file("Readme.TXT", b"Version 1".to_vec()),
        FsNode::new_file("README.txt", b"Version 2".to_vec()),
    ]);

    let mut src_bytes = vec![0u8; fs_image_size(FsKind::Ext)];
    let src_meta = ext::ExtMeta::new(src_bytes.len() as u64, Some("CASE_SRC")).unwrap();
    let mut src_io = MemRimIO::new(&mut src_bytes);
    ext::ExtFormatter::new(&mut src_io, &src_meta)
        .format(false)
        .unwrap();
    ext::ExtInjector::new(&mut src_io, &src_meta)
        .unwrap()
        .inject_tree(&mut tree)
        .unwrap();

    let mut ext_resolver = ext::ExtResolver::new(&mut src_io, &src_meta);
    let mut dst_bytes = build_empty_fs(FsKind::Ext);
    let mut dst_io = MemRimIO::new(&mut dst_bytes);
    let dst_meta = ext::ExtMeta::from_io(&mut dst_io).unwrap();
    let mut dst_injector = ext::ExtInjector::new(&mut dst_io, &dst_meta).unwrap();

    let options = CopyOptions {
        destination_case_sensitive: true,
        detect_case_collisions: true,
        ..Default::default()
    };

    let report = copy_tree(&mut ext_resolver, &mut dst_injector, "/*", &options, None).unwrap();
    assert_eq!(
        report.files_copied, 2,
        "Case-sensitive destination must copy both files"
    );
    assert_eq!(
        report.warnings.len(),
        0,
        "No case collision warnings on case-sensitive destination"
    );

    // Verify both files exist with distinct contents in EXT4 destination
    let mut dst_resolver = ext::ExtResolver::new(&mut dst_io, &dst_meta);
    let v1 = dst_resolver.read_file("/Readme.TXT").unwrap();
    let v2 = dst_resolver.read_file("/README.txt").unwrap();
    assert_eq!(v1, b"Version 1");
    assert_eq!(v2, b"Version 2");
}

#[test]
fn test_case_insensitive_destination_fat32_semantics() {
    let mut tree = FsNode::new_container(vec![
        FsNode::new_file("Readme.TXT", b"Version 1".to_vec()),
        FsNode::new_file("README.txt", b"Version 2".to_vec()),
    ]);

    let mut src_bytes = vec![0u8; fs_image_size(FsKind::Ext)];
    let src_meta = ext::ExtMeta::new(src_bytes.len() as u64, Some("CASE_FAT")).unwrap();
    let mut src_io = MemRimIO::new(&mut src_bytes);
    ext::ExtFormatter::new(&mut src_io, &src_meta)
        .format(false)
        .unwrap();
    ext::ExtInjector::new(&mut src_io, &src_meta)
        .unwrap()
        .inject_tree(&mut tree)
        .unwrap();

    // 1. Error policy: must detect collision and reject with CopyError::CaseCollision
    {
        let mut ext_resolver = ext::ExtResolver::new(&mut src_io, &src_meta);
        let mut fat_bytes = build_empty_fs(FsKind::Fat);
        let mut fat_io = MemRimIO::new(&mut fat_bytes);
        let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
        let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

        let options = CopyOptions {
            destination_case_sensitive: false,
            detect_case_collisions: true,
            overwrite_policy: OverwritePolicy::Error,
            ..Default::default()
        };

        let res = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None);
        assert!(
            matches!(res, Err(CopyError::CaseCollision { ref entry, ref existing, .. }) if entry == "README.txt" && existing == "Readme.TXT"),
            "Expected CaseCollision error on FAT32 destination, got: {:?}",
            res
        );
    }

    // 2. Skip policy: must detect collision, emit warning, skip colliding entry, and preserve first file intact
    {
        let mut ext_resolver = ext::ExtResolver::new(&mut src_io, &src_meta);
        let mut fat_bytes = build_empty_fs(FsKind::Fat);
        let mut fat_io = MemRimIO::new(&mut fat_bytes);
        let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
        let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

        let options = CopyOptions {
            destination_case_sensitive: false,
            detect_case_collisions: true,
            overwrite_policy: OverwritePolicy::Skip,
            ..Default::default()
        };

        let report = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None).unwrap();
        assert_eq!(report.files_copied, 1, "Only first file should be copied");
        assert_eq!(report.warnings.len(), 1, "Expected 1 collision warning");
        assert!(matches!(
            report.warnings[0].kind,
            CopyWarningKind::CaseCollision { .. }
        ));

        // Read back from FAT destination to confirm no data corruption
        let mut fat_resolver = fat::FatResolver::new(&mut fat_io, &fat_meta);
        let content = fat_resolver.read_file("/Readme.TXT").unwrap();
        assert_eq!(content, b"Version 1", "Original file content preserved");
    }
}

#[test]
fn test_case_insensitive_destination_exfat_semantics() {
    let mut tree = FsNode::new_container(vec![
        FsNode::new_file("Readme.TXT", b"Version 1".to_vec()),
        FsNode::new_file("README.txt", b"Version 2".to_vec()),
    ]);

    let mut src_bytes = vec![0u8; fs_image_size(FsKind::Ext)];
    let src_meta = ext::ExtMeta::new(src_bytes.len() as u64, Some("CASE_XF")).unwrap();
    let mut src_io = MemRimIO::new(&mut src_bytes);
    ext::ExtFormatter::new(&mut src_io, &src_meta)
        .format(false)
        .unwrap();
    ext::ExtInjector::new(&mut src_io, &src_meta)
        .unwrap()
        .inject_tree(&mut tree)
        .unwrap();

    let mut ext_resolver = ext::ExtResolver::new(&mut src_io, &src_meta);
    let mut exfat_bytes = build_empty_fs(FsKind::ExFat);
    let mut exfat_io = MemRimIO::new(&mut exfat_bytes);
    let exfat_meta = exfat::ExFatMeta::from_io(&mut exfat_io).unwrap();
    let mut exfat_injector = exfat::ExFatInjector::new(&mut exfat_io, &exfat_meta).unwrap();

    let options = CopyOptions {
        destination_case_sensitive: false,
        detect_case_collisions: true,
        overwrite_policy: OverwritePolicy::Error,
        ..Default::default()
    };

    let res = copy_tree(&mut ext_resolver, &mut exfat_injector, "/*", &options, None);
    assert!(
        matches!(res, Err(CopyError::CaseCollision { .. })),
        "Expected CaseCollision error on exFAT destination, got: {:?}",
        res
    );
}

#[test]
fn test_case_insensitive_destination_ntfs_semantics() {
    let mut tree = FsNode::new_container(vec![
        FsNode::new_file("Readme.TXT", b"Version 1".to_vec()),
        FsNode::new_file("README.txt", b"Version 2".to_vec()),
    ]);

    let mut src_bytes = vec![0u8; fs_image_size(FsKind::Ext)];
    let src_meta = ext::ExtMeta::new(src_bytes.len() as u64, Some("CASE_NT")).unwrap();
    let mut src_io = MemRimIO::new(&mut src_bytes);
    ext::ExtFormatter::new(&mut src_io, &src_meta)
        .format(false)
        .unwrap();
    ext::ExtInjector::new(&mut src_io, &src_meta)
        .unwrap()
        .inject_tree(&mut tree)
        .unwrap();

    let mut ext_resolver = ext::ExtResolver::new(&mut src_io, &src_meta);
    let mut ntfs_bytes = build_empty_fs(FsKind::Ntfs);
    let mut ntfs_io = MemRimIO::new(&mut ntfs_bytes);
    let ntfs_meta = ntfs::NtfsMeta::from_io(&mut ntfs_io).unwrap();
    let mut ntfs_injector = ntfs::NtfsInjector::new(&mut ntfs_io, &ntfs_meta).unwrap();

    let options = CopyOptions {
        destination_case_sensitive: false,
        detect_case_collisions: true,
        overwrite_policy: OverwritePolicy::Error,
        ..Default::default()
    };

    let res = copy_tree(&mut ext_resolver, &mut ntfs_injector, "/*", &options, None);
    assert!(
        matches!(res, Err(CopyError::CaseCollision { .. })),
        "Expected CaseCollision error on NTFS destination, got: {:?}",
        res
    );
}

#[test]
fn test_replace_policy_disabled_on_filesystem_images() {
    let mut ext_bytes = build_populated_fs(FsKind::Ext);
    let mut ext_io = MemRimIO::new(&mut ext_bytes);
    let ext_meta = ext::ExtMeta::from_io(&mut ext_io).unwrap();
    let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);

    // Test on FAT32
    let mut fat_bytes = build_empty_fs(FsKind::Fat);
    let mut fat_io = MemRimIO::new(&mut fat_bytes);
    let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
    let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

    let options = CopyOptions {
        overwrite_policy: OverwritePolicy::Replace,
        destination_supports_replace: false,
        ..Default::default()
    };

    let res = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None);
    assert!(
        matches!(res, Err(CopyError::UnsupportedFeature { ref details, .. }) if details.contains("unsupported on filesystem images")),
        "Expected Replace to be disabled on filesystem images, got: {:?}",
        res
    );
}

#[test]
fn test_host_replace_asymmetric_workload_and_mismatches() {
    let temp_src = std::env::temp_dir().join(format!("rim_host_src_{}", std::process::id()));
    let temp_dst = std::env::temp_dir().join(format!("rim_host_dst_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_src);
    let _ = fs::remove_dir_all(&temp_dst);
    fs::create_dir_all(&temp_src).unwrap();
    fs::create_dir_all(&temp_dst).unwrap();

    let file_10m = temp_src.join("data.bin");
    let payload_10m = vec![0x33u8; 10 * 1024 * 1024];
    fs::write(&file_10m, &payload_10m).unwrap();

    // 1. Initial copy of 10 MiB to host destination
    {
        let mut resolver = StdResolver::new();
        let mut injector = StdInjector::new(&temp_dst).unwrap();
        let options = CopyOptions {
            overwrite_policy: OverwritePolicy::Replace,
            destination_supports_replace: true,
            destination_case_sensitive: cfg!(unix) && !cfg!(target_os = "macos"),
            ..Default::default()
        };
        let src_file_str = file_10m.to_str().unwrap();
        copy_tree(&mut resolver, &mut injector, src_file_str, &options, None).unwrap();

        let dst_file = temp_dst.join("data.bin");
        assert_eq!(fs::metadata(&dst_file).unwrap().len(), 10 * 1024 * 1024);
    }

    // 2. Replace with 1 KiB file
    let payload_1k = vec![0x77u8; 1024];
    fs::write(&file_10m, &payload_1k).unwrap();
    {
        let mut resolver = StdResolver::new();
        let mut injector = StdInjector::new(&temp_dst).unwrap();
        let options = CopyOptions {
            overwrite_policy: OverwritePolicy::Replace,
            destination_supports_replace: true,
            destination_case_sensitive: cfg!(unix) && !cfg!(target_os = "macos"),
            ..Default::default()
        };
        let src_file_str = file_10m.to_str().unwrap();
        copy_tree(&mut resolver, &mut injector, src_file_str, &options, None).unwrap();

        let dst_file = temp_dst.join("data.bin");
        assert_eq!(fs::metadata(&dst_file).unwrap().len(), 1024);
        let read_back = fs::read(&dst_file).unwrap();
        assert_eq!(read_back, payload_1k);

        // Ensure directory has exactly 1 entry
        let entries: Vec<_> = fs::read_dir(&temp_dst).unwrap().collect();
        assert_eq!(entries.len(), 1);
    }

    // 3. Reverse: Replace 1 KiB back with 10 MiB
    fs::write(&file_10m, &payload_10m).unwrap();
    {
        let mut resolver = StdResolver::new();
        let mut injector = StdInjector::new(&temp_dst).unwrap();
        let options = CopyOptions {
            overwrite_policy: OverwritePolicy::Replace,
            destination_supports_replace: true,
            destination_case_sensitive: cfg!(unix) && !cfg!(target_os = "macos"),
            ..Default::default()
        };
        let src_file_str = file_10m.to_str().unwrap();
        copy_tree(&mut resolver, &mut injector, src_file_str, &options, None).unwrap();

        let dst_file = temp_dst.join("data.bin");
        assert_eq!(fs::metadata(&dst_file).unwrap().len(), 10 * 1024 * 1024);
    }

    // Cleanup
    let _ = fs::remove_dir_all(&temp_src);
    let _ = fs::remove_dir_all(&temp_dst);
}

#[test]
fn test_dry_run_simulation_leaves_destination_untouched() {
    // 1. Filesystem Image Test: EXT4 -> FAT32 using OverlayRimIO
    let mut ext_bytes = build_populated_fs(FsKind::Ext);
    let mut ext_io = MemRimIO::new(&mut ext_bytes);
    let ext_meta = ext::ExtMeta::from_io(&mut ext_io).unwrap();
    let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);

    let mut fat_bytes = build_empty_fs(FsKind::Fat);
    let fat_bytes_initial = fat_bytes.clone();
    let fat_len = fat_bytes.len() as u64;
    let mut fat_io = MemRimIO::new(&mut fat_bytes);
    let mut cow_io = rimio::prelude::OverlayRimIO::new(&mut fat_io, fat_len);
    let fat_meta = fat::FatMeta::from_io(&mut cow_io).unwrap();
    let mut fat_injector = fat::FatInjector::new(&mut cow_io, &fat_meta).unwrap();

    let options = CopyOptions {
        destination_case_sensitive: false,
        destination_supports_replace: false,
        ..Default::default()
    };

    let report = copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None).unwrap();

    // Assert that the simulation counted the files and bytes
    assert!(report.files_copied > 0, "Dry run must count copied files");
    assert!(
        report.bytes_transferred > 0,
        "Dry run must count transferred bytes"
    );
    assert!(
        report.directories_created > 0,
        "Dry run must count directories"
    );

    // Assert that in-memory sparse pages were allocated to simulate filesystem structures
    assert!(
        cow_io.allocated_bytes() > 0,
        "Sparse pages should be allocated in memory"
    );
    assert!(
        cow_io.allocated_pages() > 0,
        "Sparse pages should be allocated in memory"
    );

    // Assert that destination backing buffer remains 100% byte-for-byte identical (untouched)
    assert_eq!(
        fat_bytes, fat_bytes_initial,
        "Destination image bytes must not be modified during a dry-run"
    );

    // 2. Host Test: StdResolver -> DryRunStdInjector
    let temp_src = std::env::temp_dir().join(format!("rim_dry_host_src_{}", std::process::id()));
    let temp_dst = std::env::temp_dir().join(format!("rim_dry_host_dst_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_src);
    let _ = fs::remove_dir_all(&temp_dst);
    fs::create_dir_all(&temp_src).unwrap();

    fs::write(temp_src.join("file1.txt"), b"file1 content").unwrap();
    fs::create_dir_all(temp_src.join("sub")).unwrap();
    fs::write(
        temp_src.join("sub").join("file2.txt"),
        b"file2 nested content",
    )
    .unwrap();

    {
        let mut resolver = StdResolver::new();
        let mut injector = rimcli::copy::DryRunStdInjector::new(&temp_dst);
        let host_opts = CopyOptions {
            destination_case_sensitive: cfg!(unix) && !cfg!(target_os = "macos"),
            destination_supports_replace: true,
            ..Default::default()
        };

        let src_pattern = format!("{}/*", temp_src.to_str().unwrap());
        let host_report =
            copy_tree(&mut resolver, &mut injector, &src_pattern, &host_opts, None).unwrap();

        assert_eq!(host_report.files_copied, 2);
        assert_eq!(host_report.directories_created, 1);
        assert_eq!(
            host_report.bytes_transferred,
            b"file1 content".len() as u64 + b"file2 nested content".len() as u64
        );

        // Verify host destination directory was NEVER created or modified on disk!
        assert!(
            !temp_dst.exists(),
            "Host destination must not even exist on disk in dry-run mode"
        );
    }

    let _ = fs::remove_dir_all(&temp_src);
}

#[test]
fn test_multi_partition_addressing_and_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("disk.img");

    let layout = rimgen::LayoutConfig {
        base_dir: PathBuf::from("."),
        partitions: vec![
            rimgen::PartitionConfig {
                name: "PART_FAT".to_string(),
                size: rimgen::Size::Fixed(32),
                fs: rimgen::Filesystem::Fat32,
                mountpoint: None,
                index: None,
                bootable: false,
                kind: None,
                guid: None,
                payload: None,
                label: Some("P1".to_string()),
                uuid: None,
            },
            rimgen::PartitionConfig {
                name: "PART_EXT".to_string(),
                size: rimgen::Size::Fixed(32),
                fs: rimgen::Filesystem::Ext4,
                mountpoint: None,
                index: None,
                bootable: false,
                kind: None,
                guid: None,
                payload: None,
                label: Some("P2".to_string()),
                uuid: None,
            },
        ],
        disk: None,
    };

    let total_sectors = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(&layout);
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&img_path)
        .unwrap();
    file.set_len(total_sectors * 512).unwrap();
    {
        let mut io = rimio::prelude::StdRimIO::new(&mut file);
        rimgen::build_config_on_io(&layout, &mut io).unwrap();
    }
    drop(file);

    // Populate partition 1 with A.TXT
    let src_a = dir.path().join("src_a");
    fs::create_dir_all(&src_a).unwrap();
    fs::write(src_a.join("A.TXT"), b"Partition 1 FAT32 content").unwrap();

    rimcli::commands::copy::run(
        src_a.to_str().unwrap().to_string(),
        format!("{}:1:/", img_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    // Populate partition 2 with B.TXT
    let src_b = dir.path().join("src_b");
    fs::create_dir_all(&src_b).unwrap();
    fs::write(src_b.join("B.TXT"), b"Partition 2 EXT4 content").unwrap();

    rimcli::commands::copy::run(
        src_b.to_str().unwrap().to_string(),
        format!("{}:2:/", img_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    // Verify disk.img:1:/ resolves A.TXT and NOT B.TXT
    let out_a = dir.path().join("out_a");
    rimcli::commands::copy::run(
        format!("{}:1:/", img_path.display()),
        out_a.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read(out_a.join("A.TXT")).unwrap(),
        b"Partition 1 FAT32 content"
    );
    assert!(!out_a.join("B.TXT").exists());

    // Verify disk.img:2:/ resolves B.TXT and NOT A.TXT
    let out_b = dir.path().join("out_b");
    rimcli::commands::copy::run(
        format!("{}:2:/", img_path.display()),
        out_b.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read(out_b.join("B.TXT")).unwrap(),
        b"Partition 2 EXT4 content"
    );
    assert!(!out_b.join("A.TXT").exists());

    // Destination selection isolation test:
    // Snapshot partition 1 bytes
    let (p1_start, p1_size) = {
        let mut file = fs::File::open(&img_path).unwrap();
        let mut io = rimio::prelude::StdRimIO::new(&mut file);
        let scan = rimpart::scan_disk_with_sector(&mut io, 512).unwrap();
        (
            (scan.partitions[0].start_lba * 512) as usize,
            scan.partitions[0].size_bytes as usize,
        )
    };

    let img_bytes_before = fs::read(&img_path).unwrap();
    let p1_bytes_before = img_bytes_before[p1_start..p1_start + p1_size].to_vec();

    // Inject new payload into partition 2
    let src_new = dir.path().join("src_new");
    fs::create_dir_all(&src_new).unwrap();
    fs::write(src_new.join("NEW.TXT"), b"New payload for partition 2").unwrap();

    rimcli::commands::copy::run(
        src_new.to_str().unwrap().to_string(),
        format!("{}:2:/", img_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    // Verify partition 2 now contains NEW.TXT
    let out_b2 = dir.path().join("out_b2");
    rimcli::commands::copy::run(
        format!("{}:2:/", img_path.display()),
        out_b2.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read(out_b2.join("NEW.TXT")).unwrap(),
        b"New payload for partition 2"
    );

    // Verify partition 1 remained byte-for-byte identical!
    let img_bytes_after = fs::read(&img_path).unwrap();
    let p1_bytes_after = &img_bytes_after[p1_start..p1_start + p1_size];
    assert_eq!(
        p1_bytes_before.as_slice(),
        p1_bytes_after,
        "Partition 1 bytes must remain identical when writing to partition 2"
    );

    // Test missing selector on multi-partition image fails deterministically
    let out_missing = dir.path().join("out_missing");
    let err_missing = rimcli::commands::copy::run(
        img_path.display().to_string(),
        out_missing.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err_missing.contains("contains 2 partitions, but no partition was specified"),
        "Error should list missing selector: {err_missing}"
    );
    assert!(err_missing.contains("[1]"));
    assert!(err_missing.contains("[2]"));

    // Test nonexistent partition (partition 3 on 2-partition image) fails deterministically
    let err_p3 = rimcli::commands::copy::run(
        format!("{}:3:/", img_path.display()),
        out_missing.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err_p3.contains("Partition 3 does not exist in image"),
        "Error should report partition 3 nonexistence: {err_p3}"
    );

    // Test partition 0 fails deterministically
    let err_p0 = rimcli::commands::copy::run(
        format!("{}:0:/", img_path.display()),
        out_missing.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(err_p0.contains("Invalid partition number 0"));

    // Test malformed partition selector fails deterministically
    let err_bad = rimcli::commands::copy::run(
        format!("{}:abc:/", img_path.display()),
        out_missing.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(err_bad.contains("Invalid partition selector 'abc'"));
}

#[test]
fn test_unpartitioned_raw_fs_copy_and_missing_selector() {
    let dir = tempfile::tempdir().unwrap();
    let img_path = dir.path().join("rootfs.ext4");

    // Format raw unpartitioned EXT4 filesystem
    let mut bytes = vec![0u8; 32 * 1024 * 1024];
    {
        let len = bytes.len() as u64;
        let mut io = MemRimIO::new(&mut bytes);
        let meta = ext::ExtMeta::new(len, Some("ROOTFS")).unwrap();
        ext::ExtFormatter::new(&mut io, &meta)
            .format(false)
            .unwrap();
    }
    fs::write(&img_path, &bytes).unwrap();

    // Inject host folder into unpartitioned image: rootfs.ext4:/
    let src = dir.path().join("src_unpart");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("data.txt"), b"Hello unpartitioned raw disk").unwrap();

    rimcli::commands::copy::run(
        src.to_str().unwrap().to_string(),
        format!("{}:/", img_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    // Read back without partition selector (transparent unpartitioned handling)
    let out = dir.path().join("out_unpart");
    rimcli::commands::copy::run(
        format!("{}:/", img_path.display()),
        out.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    assert_eq!(
        fs::read(out.join("data.txt")).unwrap(),
        b"Hello unpartitioned raw disk"
    );

    // Specifying partition on unpartitioned image fails deterministically
    let err_part_on_raw = rimcli::commands::copy::run(
        format!("{}:1:/", img_path.display()),
        out.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(
        err_part_on_raw.contains("contains no partition table")
            || err_part_on_raw.contains("does not have a valid partition table")
    );
}

#[test]
fn test_container_copy_vhd_and_vmdk() {
    let dir = tempfile::tempdir().unwrap();

    let raw_path = dir.path().join("raw.img");
    let vhd_path = dir.path().join("disk.vhd");
    let vmdk_path = dir.path().join("disk.vmdk");

    let layout = rimgen::LayoutConfig {
        base_dir: PathBuf::from("."),
        partitions: vec![rimgen::PartitionConfig {
            name: "DATA".to_string(),
            size: rimgen::Size::Fixed(16),
            fs: rimgen::Filesystem::Fat32,
            mountpoint: None,
            index: None,
            bootable: false,
            kind: None,
            guid: None,
            payload: None,
            label: None,
            uuid: None,
        }],
        disk: None,
    };

    let total_sectors = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(&layout);
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&raw_path)
        .unwrap();
    file.set_len(total_sectors * 512).unwrap();
    {
        let mut io = rimio::prelude::StdRimIO::new(&mut file);
        rimgen::build_config_on_io(&layout, &mut io).unwrap();
    }
    drop(file);

    // Convert raw to VHD and VMDK
    rimcli::commands::convert::run(raw_path.clone(), vhd_path.clone(), 0, true).unwrap();

    rimcli::commands::convert::run(raw_path.clone(), vmdk_path.clone(), 0, true).unwrap();

    // Write to VHD container via rim copy (testing streaming container write)
    let src = dir.path().join("src_vhd");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("vhd_file.txt"), b"VHD container write payload").unwrap();

    rimcli::commands::copy::run(
        src.to_str().unwrap().to_string(),
        format!("{}:1:/", vhd_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    // Read back from VHD container via rim copy
    let out = dir.path().join("out_vhd");
    rimcli::commands::copy::run(
        format!("{}:1:/", vhd_path.display()),
        out.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read(out.join("vhd_file.txt")).unwrap(),
        b"VHD container write payload"
    );

    // Write to VMDK container via rim copy
    let src_vmdk = dir.path().join("src_vmdk");
    fs::create_dir_all(&src_vmdk).unwrap();
    fs::write(
        src_vmdk.join("vmdk_file.txt"),
        b"VMDK container write payload",
    )
    .unwrap();

    rimcli::commands::copy::run(
        src_vmdk.to_str().unwrap().to_string(),
        format!("{}:1:/", vmdk_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();

    // Read back from VMDK container
    let out_vmdk = dir.path().join("out_vmdk");
    rimcli::commands::copy::run(
        format!("{}:1:/", vmdk_path.display()),
        out_vmdk.to_str().unwrap().to_string(),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap();
    assert_eq!(
        fs::read(out_vmdk.join("vmdk_file.txt")).unwrap(),
        b"VMDK container write payload"
    );
}

#[test]
fn test_qcow2_destination_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let qcow2_path = dir.path().join("disk.qcow2");

    // Create a dummy QCOW2 header
    let mut header = [0u8; 512];
    header[..4].copy_from_slice(&[0x51, 0x46, 0x49, 0xfb]);
    fs::write(&qcow2_path, header).unwrap();

    let src = dir.path().join("src_dummy");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("dummy.txt"), b"test").unwrap();

    let err = rimcli::commands::copy::run(
        src.to_str().unwrap().to_string(),
        format!("{}:1:/", qcow2_path.display()),
        "/".to_string(),
        "preserve-all".to_string(),
        "warn".to_string(),
        None,
        false,
        false,
        0,
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(err.contains("Writing directly to QCOW2 containers as a destination is not supported"));
}

#[test]
fn test_metadata_vocabulary_and_aliases() {
    // Canonical
    assert_eq!(
        MetadataPolicy::from_str("preserve-all").unwrap(),
        MetadataPolicy::PreserveAll
    );
    assert_eq!(
        MetadataPolicy::from_str("preserve-basic").unwrap(),
        MetadataPolicy::PreserveBasic
    );
    assert_eq!(
        MetadataPolicy::from_str("strip").unwrap(),
        MetadataPolicy::Strip
    );

    // Aliases
    assert_eq!(
        MetadataPolicy::from_str("preserve").unwrap(),
        MetadataPolicy::PreserveAll
    );
    assert_eq!(
        MetadataPolicy::from_str("all").unwrap(),
        MetadataPolicy::PreserveAll
    );
    assert_eq!(
        MetadataPolicy::from_str("basic").unwrap(),
        MetadataPolicy::PreserveBasic
    );
    assert_eq!(
        MetadataPolicy::from_str("best-effort").unwrap(),
        MetadataPolicy::PreserveBasic
    );
    assert_eq!(
        MetadataPolicy::from_str("none").unwrap(),
        MetadataPolicy::Strip
    );
    assert_eq!(
        MetadataPolicy::from_str("ignore").unwrap(),
        MetadataPolicy::Strip
    );

    // Unsupported aliases
    assert_eq!(
        UnsupportedMetadataPolicy::from_str("warn").unwrap(),
        UnsupportedMetadataPolicy::Warn
    );
    assert_eq!(
        UnsupportedMetadataPolicy::from_str("warning").unwrap(),
        UnsupportedMetadataPolicy::Warn
    );
    assert_eq!(
        UnsupportedMetadataPolicy::from_str("error").unwrap(),
        UnsupportedMetadataPolicy::Error
    );
    assert_eq!(
        UnsupportedMetadataPolicy::from_str("fail").unwrap(),
        UnsupportedMetadataPolicy::Error
    );
    assert_eq!(
        UnsupportedMetadataPolicy::from_str("ignore").unwrap(),
        UnsupportedMetadataPolicy::Ignore
    );
    assert_eq!(
        UnsupportedMetadataPolicy::from_str("skip").unwrap(),
        UnsupportedMetadataPolicy::Ignore
    );

    // Overwrite aliases
    assert_eq!(
        OverwritePolicy::from_str("replace").unwrap(),
        OverwritePolicy::Replace
    );
    assert_eq!(
        OverwritePolicy::from_str("overwrite").unwrap(),
        OverwritePolicy::Replace
    );
    assert_eq!(
        OverwritePolicy::from_str("error").unwrap(),
        OverwritePolicy::Error
    );
    assert_eq!(
        OverwritePolicy::from_str("fail").unwrap(),
        OverwritePolicy::Error
    );
    assert_eq!(
        OverwritePolicy::from_str("skip").unwrap(),
        OverwritePolicy::Skip
    );
}

#[test]
fn test_host_overwrite_policies_reporting() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();

    fs::write(src.join("file.txt"), b"Source new content").unwrap();
    fs::write(dst.join("file.txt"), b"Existing destination content").unwrap();

    // 1. Error policy: fails
    let mut resolver = StdResolver::new();
    let mut injector_err = StdInjector::new(&dst)
        .unwrap()
        .with_overwrite_policy(rimfs::core::StdOverwritePolicy::Error);
    let opts_err = CopyOptions {
        overwrite_policy: OverwritePolicy::Error,
        destination_supports_replace: true,
        ..Default::default()
    };
    let src_pat = format!("{}/*", src.display());
    let err = copy_tree(&mut resolver, &mut injector_err, &src_pat, &opts_err, None).unwrap_err();
    assert!(format!("{err:?}").contains("Destination file already exists"));
    assert_eq!(
        fs::read(dst.join("file.txt")).unwrap(),
        b"Existing destination content"
    );

    // 2. Skip policy: skips, file untouched, files_skipped incremented, files_copied NOT incremented
    let mut resolver2 = StdResolver::new();
    let mut injector_skip = StdInjector::new(&dst)
        .unwrap()
        .with_overwrite_policy(rimfs::core::StdOverwritePolicy::Skip);
    let opts_skip = CopyOptions {
        overwrite_policy: OverwritePolicy::Skip,
        destination_supports_replace: true,
        ..Default::default()
    };
    let report_skip = copy_tree(
        &mut resolver2,
        &mut injector_skip,
        &src_pat,
        &opts_skip,
        None,
    )
    .unwrap();
    assert_eq!(report_skip.files_copied, 0);
    assert_eq!(report_skip.files_skipped, 1);
    assert_eq!(report_skip.bytes_transferred, 0);
    assert_eq!(
        fs::read(dst.join("file.txt")).unwrap(),
        b"Existing destination content"
    );

    // 3. Replace policy: replaces, file updated, files_copied incremented
    let mut resolver3 = StdResolver::new();
    let mut injector_replace = StdInjector::new(&dst)
        .unwrap()
        .with_overwrite_policy(rimfs::core::StdOverwritePolicy::Replace);
    let opts_replace = CopyOptions {
        overwrite_policy: OverwritePolicy::Replace,
        destination_supports_replace: true,
        ..Default::default()
    };
    let report_replace = copy_tree(
        &mut resolver3,
        &mut injector_replace,
        &src_pat,
        &opts_replace,
        None,
    )
    .unwrap();
    assert_eq!(report_replace.files_copied, 1);
    assert_eq!(report_replace.files_skipped, 0);
    assert_eq!(
        report_replace.bytes_transferred,
        b"Source new content".len() as u64
    );
    assert_eq!(
        fs::read(dst.join("file.txt")).unwrap(),
        b"Source new content"
    );
}

#[test]
fn test_std_injector_confinement_redteam() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("confinement_root");
    fs::create_dir_all(&root).unwrap();

    let mut injector = StdInjector::new(&root).unwrap();
    let attr = FileAttributes::new_file();
    let mut payload = VecRimIO::new(b"evil".to_vec());

    // 1. Rejects ".."
    assert!(
        injector
            .write_file("../escape.txt", &mut payload, 4, &attr)
            .is_err()
    );

    // 2. Rejects path separators
    assert!(
        injector
            .write_file("sub/nested.txt", &mut payload, 4, &attr)
            .is_err()
    );
    assert!(
        injector
            .write_file(r"sub\nested.txt", &mut payload, 4, &attr)
            .is_err()
    );

    // 3. Rejects NUL
    assert!(
        injector
            .write_file("null\0byte.txt", &mut payload, 4, &attr)
            .is_err()
    );

    // 4. Rejects Windows colon / NTFS Alternate Data Streams (ADS)
    assert!(
        injector
            .write_file("file.txt:evil_stream", &mut payload, 4, &attr)
            .is_err()
    );
    assert!(
        injector
            .write_file("C:drive_escape.txt", &mut payload, 4, &attr)
            .is_err()
    );

    // 5. Rejects empty entry
    assert!(injector.write_file("", &mut payload, 4, &attr).is_err());
}

#[test]
fn test_case_collision_readme_txt_matrix() {
    // Populate an EXT4 filesystem with both "Readme.TXT" and "README.txt"
    let mut ext_bytes = build_empty_fs(FsKind::Ext);
    {
        let mut ext_io = MemRimIO::new(&mut ext_bytes);
        let ext_meta = ext::ExtMeta::from_io(&mut ext_io).unwrap();
        let mut ext_injector = ext::ExtInjector::new(&mut ext_io, &ext_meta).unwrap();
        let mut tree = FsNode::new_container(vec![
            FsNode::new_file_from_source(
                "Readme.TXT",
                Box::new(VecRimIO::new(b"First file".to_vec())),
                FileAttributes::new_file(),
            ),
            FsNode::new_file_from_source(
                "README.txt",
                Box::new(VecRimIO::new(b"Colliding file".to_vec())),
                FileAttributes::new_file(),
            ),
        ]);
        ext_injector.inject_tree(&mut tree).unwrap();
    }

    // 1. FAT32: Case-insensitive destination errors on collision
    {
        let mut fat_bytes = build_empty_fs(FsKind::Fat);
        let mut fat_io = MemRimIO::new(&mut fat_bytes);
        let fat_meta = fat::FatMeta::from_io(&mut fat_io).unwrap();
        let mut fat_injector = fat::FatInjector::new(&mut fat_io, &fat_meta).unwrap();

        let mut ext_io = MemRimIO::new(&mut ext_bytes);
        let ext_meta = ext::ExtMeta::from_io(&mut ext_io).unwrap();
        let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);

        let options = CopyOptions {
            overwrite_policy: OverwritePolicy::Error,
            detect_case_collisions: true,
            destination_case_sensitive: false,
            ..Default::default()
        };

        let err =
            copy_tree(&mut ext_resolver, &mut fat_injector, "/*", &options, None).unwrap_err();
        match err {
            CopyError::CaseCollision {
                entry, existing, ..
            } => {
                assert_eq!(entry.to_lowercase(), existing.to_lowercase());
            }
            other => panic!("Expected CaseCollision error, got: {:?}", other),
        }
    }

    // 2. EXT4: Case-sensitive destination allows both files to co-exist
    {
        let mut ext_dst_bytes = build_empty_fs(FsKind::Ext);
        let mut ext_dst_io = MemRimIO::new(&mut ext_dst_bytes);
        let ext_dst_meta = ext::ExtMeta::from_io(&mut ext_dst_io).unwrap();
        let mut ext_dst_injector = ext::ExtInjector::new(&mut ext_dst_io, &ext_dst_meta).unwrap();

        let mut ext_io = MemRimIO::new(&mut ext_bytes);
        let ext_meta = ext::ExtMeta::from_io(&mut ext_io).unwrap();
        let mut ext_resolver = ext::ExtResolver::new(&mut ext_io, &ext_meta);

        let ext_options = CopyOptions {
            overwrite_policy: OverwritePolicy::Error,
            detect_case_collisions: true,
            destination_case_sensitive: true,
            ..Default::default()
        };

        let report = copy_tree(
            &mut ext_resolver,
            &mut ext_dst_injector,
            "/*",
            &ext_options,
            None,
        )
        .unwrap();
        assert_eq!(report.files_copied, 2);
    }
}
