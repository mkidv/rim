#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(
    all(not(feature = "std"), feature = "alloc"),
    feature(alloc_error_handler)
)]

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

// === Core Modules ===
pub mod core;
pub mod fs;

// Reusable types and traits
pub use core::traits::*;

// Utilities
#[cfg(feature = "std")]
pub use core::StdFsParser;
pub use core::utils::{path_utils::*, volume_utils::*};

// Filesystem APIs
pub mod fat32 {
    #[cfg(feature = "std")]
    pub use super::core::StdFsParser;
    pub use super::fs::fat32::prelude::*;
}

pub mod exfat {
    #[cfg(feature = "std")]
    pub use super::core::StdFsParser;
    pub use super::fs::exfat::prelude::*;
}
