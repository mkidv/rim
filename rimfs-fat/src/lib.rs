#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
#[macro_use]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

pub mod allocator;
pub mod attr;
pub mod boot_code;
pub mod checker;
pub mod constant;
pub mod filesystem;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod resolver;
pub mod types;
pub mod utils;

pub mod traits {
    pub use super::allocator::{FatAllocator, FatHandle};
    pub use super::checker::FatChecker;
    pub use super::formatter::FatFormatter;
    pub use super::injector::FatInjector;
    pub use super::meta::FatMeta;
    pub use super::resolver::FatResolver;
}

pub mod prelude {
    pub use super::attr::FatFileAttributesExt;
    pub use super::filesystem::Fat;
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
