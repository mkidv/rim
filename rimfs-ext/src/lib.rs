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
#[allow(dead_code)]
pub(crate) mod features;
mod filesystem;
mod formatter;
pub(crate) mod group_layout;
mod injector;
mod meta;
#[allow(dead_code)]
pub(crate) mod ops;
mod resolver;
pub mod types;
pub(crate) mod updates;
#[allow(dead_code)]
pub(crate) mod utils;

pub mod traits {
    pub use super::allocator::{ExtAllocator, ExtHandle};
    pub use super::checker::{ExtChecker, ExtCheckerOptions};
    pub use super::filesystem::Ext;
    pub use super::formatter::ExtFormatter;
    pub use super::injector::ExtInjector;
    pub use super::meta::{ExtFeatureSet, ExtMeta};
    pub use super::resolver::ExtResolver;
}

pub mod prelude {
    pub use super::attr::ExtFileAttributesExt;
    pub use super::filesystem::Ext;
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
