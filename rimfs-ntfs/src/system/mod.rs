// SPDX-License-Identifier: MIT
//! NTFS Low-Level System File Builders
//!
//! This module contains pure low-level builders that construct the binary contents,
//! index structures, and data streams for specialized system files (`$Secure`, `$Quota`, `$UpCase`).
//!
//! Architectural distinction:
//! - `crate::system`: Constructs the raw data buffers and internal index trees.
//! - `crate::features`: Implements `FsSystemFeature`, orchestrating cluster allocation,
//!   MFT record synthesis, and disk write operations during formatting.

pub mod quota;
pub mod secure;
pub mod upcase;
