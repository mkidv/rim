// SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

pub mod checker;
pub mod filesystem;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod resolver;
pub mod types;

pub use checker::{TarChecker, TarCheckerOptions};
pub use filesystem::Tar;
pub use formatter::TarFormatter;
pub use injector::TarInjector;
pub use meta::TarMeta;
pub use resolver::TarResolver;
pub use types::{TAR_BLOCK_SIZE, TarEntry, TarHandle};

pub mod traits {
    pub use super::checker::TarChecker;
    pub use super::formatter::TarFormatter;
    pub use super::injector::TarInjector;
    pub use super::meta::TarMeta;
    pub use super::resolver::TarResolver;
    pub use super::types::TarHandle;
}

pub mod prelude {
    pub use super::filesystem::Tar;
    pub use super::traits::*;
    pub use super::types::*;
    #[cfg(feature = "std")]
    pub use rimfs_core::StdResolver;
    pub use rimfs_core::errors::*;
    pub use rimfs_core::traits::*;
    pub use rimio::prelude::*;
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimfs_core::injector::FsTreeInjector;
    use rimfs_core::resolver::FsTreeResolver;
    use rimio::MemRimIO;
    use rimio::SliceRimIO;

    #[test]
    fn test_tar_roundtrip() {
        let meta = TarMeta::default();
        let mut disk_buf = [0u8; 10240];
        let mut io = MemRimIO::new(&mut disk_buf);

        let mut injector = TarInjector::new(&mut io, &meta).unwrap();

        let file1_data = b"Hello, world!";
        let mut file1_reader = SliceRimIO::new(file1_data);
        let attr = rimfs_core::resolver::attr::FileAttributes::new_file();
        injector
            .write_file(
                "hello.txt",
                &mut file1_reader,
                file1_data.len() as u64,
                &attr,
            )
            .unwrap();

        FsTreeInjector::flush(&mut injector).unwrap();

        let mut slice_io = SliceRimIO::new(&disk_buf);
        let mut resolver = TarResolver::new(&mut slice_io, &meta);
        let tree = resolver.resolve_tree("/*").unwrap();
        let counts = tree.counts();
        assert_eq!(counts.files, 1);
    }
}
