// SPDX-License-Identifier: MIT

//! Streaming data transfer between RimIO streams.

use crate::meta::FsMeta;
use rimio::prelude::*;

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
        let byte_len = run.length * meta.unit_size();
        phys_runs.push(Run::new(phys_start, byte_len));
    }

    // Use MappedRimIO with unit_size=1 (byte-addressing).
    let mut mapped = MappedRimIO::new(dest, &phys_runs, 1);
    mapped.copy_from(source, 0, 0, total_size)
}
