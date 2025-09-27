#[macro_use]
mod macros;
mod io_ext;

pub mod errors;
pub mod gpt;
pub mod guids;
pub mod mbr;
mod gpt_cursor;

#[cfg(feature = "alloc")]
pub mod scanner;
#[cfg(feature = "alloc")]
pub use scanner::scan_disk;

pub mod utils;

pub use utils::{
    detect_partition_offset_by_type_guid, truncate_image, truncate_image_custom_sector,
    validate_full_disk,
};

pub const DEFAULT_SECTOR_SIZE: u64 = 512;
