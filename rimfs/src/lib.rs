#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
#[macro_use]
extern crate alloc;

// Core Modules
pub mod core;
pub mod fs;

// Reusable types and traits
pub use core::traits::*;

// Utilities
#[cfg(feature = "std")]
pub use core::StdResolver;
pub use core::utils::{path_utils::*, volume::*};

// Filesystem APIs
#[cfg(feature = "fat32")]
/// FAT32 filesystem implementation.
///
/// See [`fat32::Fat32Formatter`], [`fat32::Fat32Allocator`], and [`fat32::Fat32Injector`].
pub mod fat32 {
    #[cfg(feature = "std")]
    pub use super::core::StdResolver;
    pub use super::fs::fat32::prelude::*;
}

#[cfg(feature = "exfat")]
/// ExFAT filesystem implementation.
///
/// See [`exfat::ExFatFormatter`], [`exfat::ExFatAllocator`], and [`exfat::ExFatInjector`].
pub mod exfat {
    #[cfg(feature = "std")]
    pub use super::core::StdResolver;
    pub use super::fs::exfat::prelude::*;
}

#[cfg(feature = "ext4")]
/// EXT4 filesystem implementation.
///
/// See [`ext4::Ext4Formatter`], [`ext4::Ext4Allocator`], and [`ext4::Ext4Injector`].
pub mod ext4 {
    #[cfg(feature = "std")]
    pub use super::core::StdResolver;
    pub use super::fs::ext4::prelude::*;
}
