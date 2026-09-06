#![cfg_attr(not(feature = "std"), no_std)]

pub use core::errors::*;
pub use rimfs_core as core;
pub use rimfs_core::{bail, ensure};

// Reusable types and traits
pub use core::traits::*;

// Utilities
#[cfg(feature = "std")]
pub use core::StdResolver;
#[cfg(feature = "alloc")]
pub use core::utils::path_utils::*;
pub use core::utils::volume::*;
#[cfg(feature = "std")]
pub use core::{StdInjector, StdOverwritePolicy};

// Filesystem APIs
#[cfg(feature = "fat")]
pub use rimfs_fat as fat;

#[cfg(feature = "exfat")]
pub use rimfs_exfat as exfat;

#[cfg(feature = "ext")]
pub use rimfs_ext as ext;

#[cfg(feature = "ntfs")]
pub use rimfs_ntfs as ntfs;

#[cfg(feature = "tar")]
pub use rimfs_tar as tar;

#[cfg(feature = "zip")]
pub use rimfs_zip as zip;

#[cfg(feature = "iso")]
pub use rimfs_iso as iso;
