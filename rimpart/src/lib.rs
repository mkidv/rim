pub mod error;
pub mod gpt;
pub mod guids;
#[macro_use]
mod macros;
pub mod mbr;
pub mod types;
pub mod utils;

#[allow(clippy::single_component_path_imports)]
use paste;
#[allow(clippy::single_component_path_imports)]
use rimio;

pub const DEFAULT_SECTOR_SIZE: u64 = 512;
