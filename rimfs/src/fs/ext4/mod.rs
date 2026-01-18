// SPDX-License-Identifier: MIT
pub mod allocator;
pub mod attr;
pub mod checker;
pub mod constant;
pub mod filesystem;
pub mod formatter;
pub mod group_layout;
pub mod injector;
pub mod meta;
pub mod resolver;
pub mod types;
pub mod utils;

// Public Interface
pub mod traits {
    pub use super::allocator::{
        Ext4Allocator, Ext4BlockAllocator, Ext4Handle, Ext4MetadataAllocator,
    };
    pub use super::checker::Ext4Checker;
    pub use super::filesystem::Ext4;
    pub use super::formatter::Ext4Formatter;
    pub use super::injector::Ext4Injector;
    pub use super::meta::Ext4Meta;
    pub use super::resolver::Ext4Resolver;
}

pub mod prelude {
    pub use super::filesystem::Ext4;
    pub use super::traits::*;
    #[cfg(feature = "std")]
    pub use crate::core::StdResolver;
    pub use crate::core::errors::*;
    pub use crate::core::traits::*;
    pub use rimio::prelude::*;
}
