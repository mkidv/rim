//! rimfs-tar: POSIX USTAR and GNU longlink TAR archive reader and writer.

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
#[path = "tests/mod.rs"]
mod tests;

pub mod header;
