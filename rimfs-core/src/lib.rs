#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

mod macros;

pub mod allocator;
pub mod bitmap;
pub mod checker;
pub mod cursor;
pub mod errors;
pub mod ext;
pub mod fat;
pub mod feature;
pub mod filesystem;
pub mod formatter;
#[cfg(feature = "alloc")]
pub mod injector;
pub mod meta;
#[cfg(feature = "alloc")]
pub mod resolver;
#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub mod testing;
pub mod utils;
pub mod validate;

pub mod traits {
    pub use super::allocator::{FsAllocator, FsHandle};
    pub use super::checker::FsChecker;
    pub use super::feature::FsSystemFeature;
    pub use super::filesystem::FsFilesystem;
    pub use super::formatter::FsFormatter;
    #[cfg(feature = "alloc")]
    pub use super::injector::{FsContext, FsInjector, FsTreeInjector};
    pub use super::meta::FsMeta;
    #[cfg(feature = "alloc")]
    pub use super::resolver::{
        FsNode, FsResolver, FsTreeResolver, attr::FileAttributes, attr::NodeKind,
    };
    pub use super::validate::Validate;
}

pub use errors::*;
pub use time;
#[cfg(feature = "alloc")]
pub use utils::path_utils::*;
pub use utils::{time_utils::*, volume::*};

#[cfg(feature = "std")]
pub use injector::std_injector::{StdInjector, StdOverwritePolicy};
#[cfg(feature = "std")]
pub use resolver::std_resolver::StdResolver;
