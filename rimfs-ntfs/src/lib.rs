#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

pub mod allocator;
pub mod attr;
pub mod attrdef;
pub mod bitmap;
pub mod builder;
pub mod checker;
pub mod constant;
pub mod filesystem;
pub mod flags;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod mft;
pub mod resolver;
pub mod system;
pub mod tests;
pub mod types;
pub mod utils;
pub mod view;

pub use self::system::upcase;

pub mod traits {
    pub use super::allocator::{NtfsAllocator, NtfsHandle};
    pub use super::checker::NtfsChecker;
    pub use super::formatter::NtfsFormatter;
    pub use super::injector::NtfsInjector;
    pub use super::meta::NtfsMeta;
    pub use super::mft::{MftAllocator, MftHandle};
    pub use super::resolver::NtfsResolver;
}

pub mod prelude {
    pub use super::filesystem::Ntfs;
    pub use super::traits::*;
    pub use super::view::attr_view::*;
    pub use super::view::mft_view::*;
    pub use super::view::runlist::*;
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
