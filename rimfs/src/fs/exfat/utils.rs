// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;

use crate::{
    core::{resolver::*, utils::time_utils},
    fs::exfat::meta::*,
};
use rimio::prelude::*;
use time::OffsetDateTime;

/// Encode ExFAT datetime (as `u32 + u8 + u8`):
/// - `u32` = date+time (same layout as FAT32)
/// - `u8` = 10ms increment (0–199)
/// - `u8` = UTC offset in minutes from -16h to +15:45h (in steps of 15 min)
pub fn datetime_from(ts: OffsetDateTime) -> (u32, u8, u8) {
    let year = ts.year().clamp(1980, 2107) as u32;
    let month = ts.month() as u32;
    let day = ts.day() as u32;
    let hour = ts.hour() as u32;
    let minute = ts.minute() as u32;
    let second = ts.second() as u32;

    let date = ((year - 1980) << 25) | (month << 21) | (day << 16);
    let time = (hour << 11) | (minute << 5) | (second / 2);
    let encoded = date | time;

    let millis_10ms = (ts.millisecond() / 10) as u8;

    let offset = ts.offset().whole_minutes();
    let utc_offset_15min = (offset / 15).clamp(-64, 63);
    let utc_encoded = if utc_offset_15min < 0 {
        ((!(-utc_offset_15min) as u8) + 1) & 0x7F // two's complement
    } else {
        utc_offset_15min as u8
    };

    (encoded, millis_10ms, utc_encoded)
}

/// Get datetime from attribute or fallback to now
pub fn datetime_from_attr(attr: &FileAttributes) -> (u32, u8, u8) {
    let ts = attr.modified.unwrap_or_else(time_utils::now_utc);
    datetime_from(ts)
}

pub fn datetime_now() -> (u32, u8, u8) {
    let ts = time_utils::now_utc();
    datetime_from(ts)
}

pub fn write_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    clusters: &[u32],
) -> RimIOResult {
    let clusters_count = meta.bitmap_clusters() as usize;
    let cs = meta.unit_size();
    let mut bitmap = vec![0u8; clusters_count * cs];
    for i in 0..clusters_count {
        let off = meta.unit_offset(meta.bitmap_cluster + i as u32);
        io.read_block_best_effort(off, &mut bitmap[i * cs..(i + 1) * cs], cs)?;
    }

    // Flip bits
    for &cluster in clusters {
        let (byte_index, bit_mask) = meta.bitmap_entry_offset(cluster);
        if byte_index < bitmap.len() {
            bitmap[byte_index] |= bit_mask;
        }
    }

    // Write back
    for i in 0..clusters_count {
        let off = meta.unit_offset(meta.bitmap_cluster + i as u32);
        io.write_block_best_effort(off, &bitmap[i * cs..(i + 1) * cs], cs)?;
    }
    Ok(())
}
