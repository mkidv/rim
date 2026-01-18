mod macros;

// Sub-modules
pub mod allocator;
pub mod checker;
pub mod cursor;
pub mod errors;
pub mod filesystem;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod resolver;
pub mod utils;

pub mod fat;
pub mod validate;

// Core Traits
pub mod traits {
    pub use super::allocator::{FsAllocator, FsHandle};
    pub use super::checker::FsChecker;
    pub use super::filesystem::FsFilesystem;
    pub use super::formatter::FsFormatter;
    pub use super::injector::{FsContext, FsNodeInjector};
    pub use super::meta::FsMeta;
    pub use super::resolver::{FsNode, FsResolver, attr::FileAttributes};
    pub use super::validate::Validate;
}

// Error types
pub use errors::*;

// Utilities
pub use utils::{path_utils::*, time_utils::*, volume::*};

// Standard-only extensions
#[cfg(feature = "std")]
pub use resolver::std_resolver::StdResolver;
