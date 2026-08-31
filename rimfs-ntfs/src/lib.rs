#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
#[macro_use]
extern crate alloc;

pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

mod allocator;
#[allow(dead_code)]
mod attr;
#[allow(dead_code)]
pub(crate) mod attrdef;
#[allow(dead_code)]
pub(crate) mod bitmap;
#[allow(dead_code, unused_imports)]
pub(crate) mod builder;
mod checker;
#[allow(dead_code)]
pub(crate) mod constant;
mod filesystem;
mod flags;
mod formatter;
mod injector;
mod meta;
mod mft;
mod resolver;
#[allow(dead_code)]
pub(crate) mod system;
#[cfg(test)]
mod tests;
pub mod types;
#[allow(dead_code)]
pub(crate) mod utils;
pub mod view;

pub use self::system::upcase;
pub use utils::apply_usa_fixup;

pub mod traits {
    pub use super::allocator::{NtfsAllocator, NtfsHandle};
    pub use super::attr::NtfsFileAttributesExt;
    pub use super::checker::{NtfsChecker, NtfsCheckerOptions};
    pub use super::formatter::NtfsFormatter;
    pub use super::injector::NtfsInjector;
    pub use super::meta::NtfsMeta;
    pub use super::mft::{MftAllocator, MftHandle};
    pub use super::resolver::NtfsResolver;
}

pub mod prelude {
    pub use super::attr::NtfsFileAttributesExt;
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
