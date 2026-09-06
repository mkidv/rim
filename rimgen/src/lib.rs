// SPDX-License-Identifier: MIT

//! `rimgen`: Declarative disk image generation engine.
//!
//! Provides the core declarative builder for creating partitioned and formatted
//! disk images from a layout description (`DiskLayout`).

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod builder;
pub mod errors;
pub mod guid;
pub mod layout;
pub mod macros;

pub use builder::{
    BuildEvent, BuildOptions, BuildReport, PartitionReport, PartitionTable, build_on_io,
    build_on_io_with_events, build_on_io_with_options, build_on_io_with_options_and_events,
    calculate_total_disk_sectors_with_options,
};
#[cfg(feature = "std")]
pub use builder::{
    build_config_on_io, build_config_on_io_with_events, build_config_on_io_with_options,
    build_config_on_io_with_options_and_events,
    calculate_total_disk_sectors_from_config_with_options,
};
pub use errors::{GenError, GenResult, LayoutError, LayoutResult};
#[cfg(feature = "std")]
pub use guid::RandomGuidGenerator;
pub use guid::{GuidGenerator, ManualGuidGenerator, SeededGuidGenerator};
pub use layout::{
    DiskConfig, Filesystem, Layout, LayoutConfig, Partition, PartitionConfig, PartitionKind, Size,
};
