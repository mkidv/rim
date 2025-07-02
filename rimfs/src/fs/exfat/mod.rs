pub mod allocator;
pub mod attr;
pub mod checker;
pub mod constant;
pub mod filesystem;
pub mod formatter;
pub mod injector;
pub mod meta;
pub mod parser;
pub mod types;
pub mod utils;

// === Public Interface ===
pub mod traits {
    pub use super::allocator::{ExFatAllocator, ExFatHandle};
    pub use super::checker::ExFatChecker;
    pub use super::formatter::ExFatFormatter;
    pub use super::injector::ExFatInjector;
    pub use super::meta::ExFatMeta;
    pub use super::parser::ExFatParser;
}

pub mod prelude {
    pub use super::filesystem::ExFat;
    pub use super::traits::*;
    #[cfg(feature = "std")]
    pub use crate::core::StdFsParser;
    pub use crate::core::error::*;
    pub use crate::core::traits::*;
    pub use rimio::prelude::*;
}