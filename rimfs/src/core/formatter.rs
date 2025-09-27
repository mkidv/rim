// SPDX-License-Identifier: MIT

pub use crate::core::errors::{FsFormatterError, FsFormatterResult};

/// A Formatter for a filesystem type.
///
/// Implementations encapsulate all required state (I/O backend, allocator, metadata).
/// Used to prepare  the low-level structure of a filesystem on the target I/O backend,
///
/// The formatter must perform a *full format* if `full_format` is `true`,
/// or a quick format otherwise.
///
pub trait FsFormatter {
    /// Format the filesystem.
    ///
    /// - `full_format`: if `true`, perform a full format, else a quick format (if supported)
    fn format(&mut self,
        full_format: bool,
    ) -> FsFormatterResult;
}
