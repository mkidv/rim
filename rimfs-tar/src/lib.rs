// SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

mod checker;
mod filesystem;
mod formatter;
mod injector;
mod meta;
mod resolver;
pub mod types;

pub use checker::{TarChecker, TarCheckerOptions};
pub use filesystem::Tar;
pub use formatter::TarFormatter;
pub use injector::TarInjector;
pub use meta::TarMeta;
pub use resolver::TarResolver;
pub use types::{TAR_BLOCK_SIZE, TarEntry, TarHandle};

pub mod traits {
    pub use super::checker::{TarChecker, TarCheckerOptions};
    pub use super::formatter::TarFormatter;
    pub use super::injector::TarInjector;
    pub use super::meta::TarMeta;
    pub use super::resolver::TarResolver;
    pub use super::types::{TAR_BLOCK_SIZE, TarHandle};
}

pub mod prelude {
    pub use super::filesystem::Tar;
    pub use super::traits::*;
    #[cfg(feature = "std")]
    pub use rimfs_core::StdResolver;
    pub use rimfs_core::errors::*;
    pub use rimfs_core::traits::*;
    pub use rimio::prelude::*;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimfs_core::checker::{FsChecker, VerifyReport};
    use rimfs_core::injector::FsTreeInjector;
    use rimfs_core::testing::{
        ExpectedFile, ExpectedLink, assert_dirs, assert_files, assert_has_error,
        assert_no_findings, assert_symlinks, basic_tree,
    };
    use rimio::SliceRimIO;
    use rimio::{MemRimIO, RimRead, RimWrite};

    #[test]
    fn test_tar_roundtrip() {
        let meta = TarMeta::default();
        let mut disk_buf = [0u8; 10240];
        let mut io = MemRimIO::new(&mut disk_buf);

        let mut tree = basic_tree();
        let mut injector = TarInjector::new(&mut io, &meta).unwrap();
        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut checker = TarChecker::new(&mut io, &meta);
        let mut report = VerifyReport::default();
        checker
            .check_content(&TarCheckerOptions, &mut report)
            .unwrap();
        assert_no_findings(&report);

        let mut slice_io = SliceRimIO::new(&disk_buf);
        let mut resolver = TarResolver::new(&mut slice_io, &meta);
        assert_dirs(&mut resolver, &["subdir"]);
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

        // Verify resolver finds both entries and reads payloads correctly
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
}
