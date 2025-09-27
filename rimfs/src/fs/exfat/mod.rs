pub mod allocator;
pub mod attr;
pub mod checker;
pub mod constant;
pub mod filesystem;
pub mod formatter;
pub mod injector;
pub mod meta;
mod ops;
pub mod resolver;
pub mod types;
pub mod upcase;
pub mod utils;

// === Public Interface ===
pub mod traits {
    pub use super::allocator::{ExFatAllocator, ExFatHandle};
    pub use super::checker::ExFatChecker;
    pub use super::formatter::ExFatFormatter;
    pub use super::injector::ExFatInjector;
    pub use super::meta::ExFatMeta;
    pub use super::resolver::ExFatResolver;
}

pub mod prelude {
    pub use super::filesystem::ExFat;
    pub use super::traits::*;
    #[cfg(feature = "std")]
    pub use crate::core::StdResolver;
    pub use crate::core::errors::*;
    pub use crate::core::traits::*;
    pub use rimio::prelude::*;
}
