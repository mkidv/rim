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
}
