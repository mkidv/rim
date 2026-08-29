#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
#[macro_use]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

pub mod allocator;
pub mod attr;
pub mod checker;
pub mod constant;
pub mod features;
pub mod filesystem;
pub mod formatter;
pub mod group_layout;
pub mod injector;
pub mod meta;
pub mod ops;
pub mod resolver;
pub mod types;
pub mod updates;
pub mod utils;

pub mod traits {
    pub use super::allocator::{ExtAllocator, ExtHandle};
    pub use super::checker::ExtChecker;
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
