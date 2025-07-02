// === Sub-modules ===
pub mod allocator;
pub mod checker;
pub mod error;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod parser;
pub mod utils;
pub mod filesystem;

// === Core Traits ===
pub mod traits {
    pub use super::allocator::{FsAllocator, FsHandle};
    pub use super::checker::FsChecker;
    pub use super::formatter::FsFormatter;
    pub use super::injector::{FsNodeInjector, FsContext};
    pub use super::parser::{FsParser, FsNode, attr::FileAttributes};
    pub use super::meta::FsMeta;
    pub use super::filesystem::FsFilesystem;
}

// === Error types ===
pub use error::*;

// === Utilities ===
pub use utils::{volume_utils::*, path_utils::*, time_utils::*};

// === Standard-only extensions ===
#[cfg(feature = "std")]
pub use parser::parser_std::StdFsParser;