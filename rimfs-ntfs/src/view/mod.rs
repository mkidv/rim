// SPDX-License-Identifier: MIT

//! In-memory typed views for inspecting raw NTFS records and attributes.

pub mod attr_view;
pub use attr_view::AttrViewError;
pub mod mft_view;
pub mod runlist;
pub use mft_view::MftViewError;
