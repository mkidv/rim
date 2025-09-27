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
pub mod utils;

// === Public Interface ===
pub mod traits {
    pub use super::allocator::{Fat32Allocator, Fat32Handle};
    pub use super::checker::Fat32Checker;
    pub use super::formatter::Fat32Formatter;
    pub use super::injector::Fat32Injector;
    pub use super::meta::Fat32Meta;
    pub use super::resolver::Fat32Resolver;
}

pub mod prelude {
    pub use super::filesystem::Fat32;
    pub use super::traits::*;
    #[cfg(feature = "std")]
    pub use crate::core::StdResolver;
    pub use crate::core::errors::*;
    pub use crate::core::traits::*;
    pub use rimio::prelude::*;
}
