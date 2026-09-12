//! rimfs-zip: ZIP archive reader and writer.

// SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

mod allocator;
mod checker;
mod filesystem;
mod formatter;
mod injector;
mod meta;
mod resolver;
pub mod types;

pub use allocator::ZipAllocator;
pub use checker::{ZipChecker, ZipCheckerOptions};
pub use filesystem::Zip;
pub use formatter::ZipFormatter;
pub use injector::ZipInjector;
pub use meta::ZipMeta;
pub use resolver::ZipResolver;
pub use types::{ZipEntry, ZipHandle};

pub mod traits {
    pub use super::allocator::ZipAllocator;
    pub use super::checker::{ZipChecker, ZipCheckerOptions};
    pub use super::formatter::ZipFormatter;
    pub use super::injector::ZipInjector;
    pub use super::meta::ZipMeta;
    pub use super::resolver::ZipResolver;
    pub use super::types::ZipHandle;
}

pub mod prelude {
    pub use super::filesystem::Zip;
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

pub mod headers;
