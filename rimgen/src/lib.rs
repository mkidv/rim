// SPDX-License-Identifier: MIT

//! `rimgen`: Declarative disk image generation engine.
//!
//! Provides the core declarative builder for creating partitioned and formatted
//! disk images from a layout description (`DiskLayout`).

#[macro_use]
pub mod macros;
pub mod builder;
pub mod errors;
pub mod layout;
pub mod utils;

pub use builder::{
    BuildEvent, BuildReport, DryRunMode, ImageBuilder, PartitionReport, build_image,
    build_image_with_events, build_on_io, build_on_io_with_events, build_raw,
    build_raw_with_events,
};
pub use errors::{GenError, GenResult, LayoutError, LayoutResult};
pub use layout::{
    DiskConfig, Filesystem, Layout, Layout as DiskLayout, Partition, PartitionKind, Size,
};
