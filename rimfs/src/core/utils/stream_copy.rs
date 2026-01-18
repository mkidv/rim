// SPDX-License-Identifier: MIT

use crate::core::meta::FsMeta;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
use rimio::{RimIO, RimIOExt};

/// Writes data from a source stream to a sequence of value units (clusters/blocks) on the destination.
///
/// This function handles the logic of iterating over a disjoint list of units, calculating
/// their physical offsets using `meta`, and copying chunks of data from `source` to `dest`.
///
/// # Arguments
///
/// * `dest` - The destination IO (the filesystem implementation).
/// * `meta` - The metadata provider to calculate unit offsets.
/// * `source` - The source IO containing the file content.
/// * `units` - The list of units (clusters/blocks) allocated for the file.
/// * `total_size` - The total size in bytes to write.
///
/// # Returns
///
pub fn write_stream_to_units<IO, M, U>(
    dest: &mut IO,
    meta: &M,
    source: &mut (dyn RimIO + '_),
    units: &[U],
    total_size: u64,
) -> rimio::prelude::RimIOResult<()>
where
    IO: RimIO + ?Sized,
    M: FsMeta<U>,
    U: Copy + Ord,
{
    let unit_size = meta.unit_size() as u64;
    let mut remaining = total_size;
    let mut src_offset = 0;

    // Allocate a buffer once to avoid allocation churn in copy_from
    // Use unit_size or a reasonable chunk size (e.g. 16KB) to balance memory usage
    let buf_size = unit_size.min(16 * 1024) as usize;
    let mut buf = vec![0u8; buf_size];

    for &unit in units {
        if remaining == 0 {
            break;
        }

        let dst_offset = meta.unit_offset(unit);
        let to_copy = remaining.min(unit_size);

        dest.copy_from_using_buffer(source, src_offset, dst_offset, to_copy, &mut buf)?;

        remaining -= to_copy;
        src_offset += to_copy;
    }

    Ok(())
}
