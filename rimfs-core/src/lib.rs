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
pub mod injector;
pub mod meta;
pub mod resolver;
pub mod utils;
pub mod validate;

pub mod traits {
    pub use super::allocator::{FsAllocator, FsHandle};
    pub use super::checker::FsChecker;
    pub use super::feature::FsSystemFeature;
    pub use super::filesystem::FsFilesystem;
    pub use super::formatter::FsFormatter;
    pub use super::injector::{FsContext, FsInjector, FsTreeInjector};
    pub use super::meta::FsMeta;
    pub use super::resolver::{FsNode, FsResolver, FsTreeResolver, attr::FileAttributes};
    pub use super::validate::Validate;
}

pub use errors::*;
pub use utils::{path_utils::*, time_utils::*, volume::*};

#[cfg(feature = "std")]
pub use resolver::std_resolver::StdResolver;
