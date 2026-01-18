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
    #[must_use = "format result must be checked for errors"]
    fn format(&mut self, full_format: bool) -> FsFormatterResult;

    /// Flush any buffered writes to disk.
    #[must_use = "flush result must be checked for errors"]
    fn flush(&mut self) -> FsFormatterResult<()> {
        Ok(())
    }
}

use crate::core::meta::FsMeta;
use rimio::{RimIO, RimIOExt};

/// Helper to zero out the data region (cluster heap) of a filesystem.
pub fn zero_cluster_heap<M: FsMeta<u32>, IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &M,
) -> FsFormatterResult {
    let first = meta.first_data_unit();
    let last = meta.last_data_unit();
    if first > last {
        return Ok(());
    }

    let start = meta.unit_offset(first);
    let end = meta.unit_offset(last) + meta.unit_size() as u64;
    let len = end.saturating_sub(start) as usize;

    io.zero_fill(start, len)?;
    Ok(())
}
