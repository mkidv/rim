//! rimfs-iso: ISO 9660, Joliet, Rock Ridge, and El Torito image reader and writer.

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
mod layout;
mod meta;
mod resolver;
pub mod types;

pub use allocator::IsoAllocator;
pub use checker::{IsoChecker, IsoCheckerOptions};
pub use filesystem::Iso;
pub use formatter::IsoFormatter;
pub use injector::IsoInjector;
pub use meta::IsoMeta;
pub use resolver::{ElToritoBootEntry, IsoResolvedEntry, IsoResolver};
pub use types::{ISO_SECTOR_SIZE, IsoHandle};

pub mod traits {
    pub use super::allocator::IsoAllocator;
    pub use super::checker::{IsoChecker, IsoCheckerOptions};
    pub use super::formatter::IsoFormatter;
    pub use super::injector::IsoInjector;
    pub use super::meta::IsoMeta;
    pub use super::resolver::{ElToritoBootEntry, IsoResolver};
    pub use super::types::{ISO_SECTOR_SIZE, IsoHandle};
}

pub mod prelude {
    pub use super::filesystem::Iso;
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

pub mod records;
