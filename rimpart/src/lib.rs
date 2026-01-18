#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[macro_use]
mod macros;
mod io_ext;

pub mod errors;
/// GUID Partition Table (GPT) implementation.
pub mod gpt;
/// Streaming GPT reader for memory-constrained environments.
pub mod gpt_stream;
/// Common Partition Type GUIDs.
pub mod guids;
/// Master Boot Record (MBR) and Protective MBR implementation.
pub mod mbr;

#[cfg(feature = "alloc")]
pub mod scanner;
#[cfg(feature = "alloc")]
pub use scanner::{scan_disk, scan_disk_with_sector};

pub mod utils;

#[cfg(feature = "alloc")]
pub use utils::{
    detect_partition_offset_by_type_guid, truncate_image, truncate_image_custom_sector,
    validate_full_disk,
};
#[cfg(not(feature = "alloc"))]
pub use utils::{truncate_image, truncate_image_custom_sector};

pub const DEFAULT_SECTOR_SIZE: u64 = 512;
