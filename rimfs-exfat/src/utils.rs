// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;

use rimio::prelude::*;

use crate::{
    core::{
        bitmap::BitmapOps,
        resolver::*,
        utils::time_utils::{self, TimeConversion},
    },
    meta::*,
};

/// Get datetime from attribute or fallback to now
pub fn datetime_from_attr(attr: &FileAttributes) -> (u32, u8, u8) {
    let ts = attr.modified.unwrap_or_else(time_utils::now_utc);
    ts.to_exfat_datetime()
}

pub fn datetime_now() -> (u32, u8, u8) {
    let ts = time_utils::now_utc();
    ts.to_exfat_datetime()
}

pub fn write_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    clusters: &RunList,
) -> RimIOResult {
    let clusters_count = meta.bitmap_clusters() as usize;
    let cs = meta.unit_size();
    let mut bitmap = vec![0u8; clusters_count * cs];
    for i in 0..clusters_count {
        let off = meta.unit_offset(meta.bitmap_cluster + i as u32);
        io.read_block_best_effort(off, &mut bitmap[i * cs..(i + 1) * cs], cs)?;
    }

    // Flip bits using optimized generic set_bits_in_range
    // Robustness: Use meta.bitmap_entry_offset to determine start bit,
    // ensuring we respect the filesystem's internal cluster-to-bit logic.
    for run in clusters.iter() {
        let start_cluster = run.start as u32;

        // Calculate bit offset using meta's logic
        let (byte_index, bit_mask) = meta.bitmap_entry_offset(start_cluster);
        let bit_offset_start = byte_index * 8 + bit_mask.trailing_zeros() as usize;

        // Check if start is within bounds (robustness check similar to original)
        // If the start itself is out of bounds, we skip.
        // set_bits_in_range will handle the end bound clamping.
        if byte_index < bitmap.len() {
            let count = run.length as usize;
            bitmap.set_bits_in_range(bit_offset_start, bit_offset_start + count, true);
        }
    }

    // Write back
    for i in 0..clusters_count {
        let off = meta.unit_offset(meta.bitmap_cluster + i as u32);
        io.write_block_best_effort(off, &bitmap[i * cs..(i + 1) * cs], cs)?;
    }
    Ok(())
}
