// SPDX-License-Identifier: MIT

use crate::meta::FsMeta;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
use rimio::prelude::*;

/// Writes data from a source stream to a sequence of value units (clusters/blocks) on the destination.
///
/// This function handles the logic of iterating over a disjoint list of units, calculating
/// their physical offsets using `meta`, and copying chunks of data from `source` to `dest`.
pub fn write_stream_to_units<IO, M, U>(
    dest: &mut IO,
    meta: &M,
    source: &mut (dyn RimRead + '_),
    units: &[U],
    total_size: u64,
) -> RimIOResult<()>
where
    IO: RimIO + ?Sized,
    M: FsMeta<U>,
    U: Copy + Ord + Into<u64>,
{
    let unit_size = meta.unit_size() as u64;
    let mut remaining = total_size;
    let mut src_offset = 0;

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

    if remaining > 0 {
        return Err(RimIOError::OutOfBounds);
    }

    Ok(())
}

/// Writes data from a source stream to a RunList on the destination.
///
/// This function is more efficient than `write_stream_to_units` as it batches I/O by runs.
pub fn write_stream_to_run_list<IO, M, U>(
    dest: &mut IO,
    meta: &M,
    source: &mut (dyn RimRead + '_),
    runs: &RunList,
    total_size: u64,
) -> RimIOResult<()>
where
    IO: RimIO + ?Sized,
    M: FsMeta<U>,
    U: Copy + Ord + TryFrom<u64>,
{
    // Convert logical unit runs (clusters) to physical byte runs.
    let mut phys_runs = RunList::new();
    for run in runs.iter() {
        let u = U::try_from(run.start).map_err(|_| RimIOError::OutOfBounds)?;
        let phys_start = meta.unit_offset(u);
        let byte_len = run.length * meta.unit_size() as u64;
        phys_runs.push(Run::new(phys_start, byte_len));
    }

    // Use MappedRimIO with unit_size=1 (byte-addressing).
    let mut mapped = MappedRimIO::new(dest, &phys_runs, 1);
    mapped.copy_from(source, 0, 0, total_size)
}
