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

pub use builder::{BuildEvent, BuildReport, PartitionReport, build_on_io, build_on_io_with_events};
#[cfg(feature = "std")]
pub use builder::{build_config_on_io, build_config_on_io_with_events};
pub use errors::{GenError, GenResult, LayoutError, LayoutResult};
#[cfg(feature = "std")]
pub use guid::RandomGuidGenerator;
pub use guid::{GuidGenerator, ManualGuidGenerator, SeededGuidGenerator};
pub use layout::{
    DiskConfig, Filesystem, Layout, LayoutConfig, Partition, PartitionConfig, PartitionKind, Size,
};
