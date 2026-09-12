// SPDX-License-Identifier: MIT

//! rimfs-exfat: exFAT (Extended File Allocation Table) driver.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
#[macro_use]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

mod allocator;
mod attr;
mod checker;
#[allow(dead_code)]
pub(crate) mod constant;
pub mod features;
mod filesystem;
mod formatter;
mod injector;
mod meta;
mod resolver;
pub mod types;
mod upcase;
#[allow(dead_code)]
pub(crate) mod utils;

pub mod traits {
    pub use super::allocator::{ExFatAllocator, ExFatHandle};
    pub use super::checker::{ExFatChecker, ExFatCheckerOptions};
    pub use super::filesystem::ExFat;
    pub use super::formatter::ExFatFormatter;
    pub use super::injector::ExFatInjector;
    pub use super::meta::ExFatMeta;
    pub use super::resolver::ExFatResolver;
}

pub mod prelude {
    pub use super::attr::ExFatFileAttributesExt;
    pub use super::filesystem::ExFat;
    pub use super::traits::*;
    #[cfg(feature = "std")]
    pub use rimfs_core::StdResolver;
    pub use rimfs_core::errors::*;
    pub use rimfs_core::traits::*;
    pub use rimio::prelude::*;
}

pub use prelude::*;
#[cfg(feature = "std")]
pub use rimfs_core::StdResolver;
pub use rimfs_core::utils::{path_utils::*, volume::*};

#[cfg(test)]
mod tests;
