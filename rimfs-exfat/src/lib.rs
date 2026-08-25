#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

pub mod allocator;
pub mod attr;
pub mod checker;
pub mod constant;
pub mod filesystem;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod resolver;
pub mod types;
pub mod upcase;
pub mod utils;

pub mod traits {
    pub use super::allocator::{ExFatAllocator, ExFatHandle};
    pub use super::checker::ExFatChecker;
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
