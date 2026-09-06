// SPDX-License-Identifier: MIT
//! NTFS time conversion helpers.
//!
//! NTFS FILETIME measures time in 100-nanosecond intervals since January 1, 1601 UTC.

/// Offset between 1601-01-01 and 1970-01-01 in 100-ns intervals
pub const FILETIME_UNIX_DIFF: u64 = 116444736000000000;

/// Get current time as NTFS FILETIME
///
/// FILETIME is 100-nanosecond intervals since January 1, 1601 UTC.
pub fn current_ntfs_time() -> u64 {
    #[cfg(feature = "std")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};

        if let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let ticks = duration.as_nanos() / 100;
            return ticks as u64 + FILETIME_UNIX_DIFF;
        }
    }

    // Default fallback: 2024-01-01 00:00:00 UTC
    133477536000000000
}

/// Convert NTFS FILETIME (100-ns intervals since 1601-01-01) to time::OffsetDateTime
pub fn ntfs_time_to_offset_date_time(filetime: u64) -> Option<time::OffsetDateTime> {
    if filetime < FILETIME_UNIX_DIFF {
        return None;
    }
    let nanos_since_epoch = (filetime - FILETIME_UNIX_DIFF) * 100;
    time::OffsetDateTime::from_unix_timestamp_nanos(nanos_since_epoch as i128).ok()
}
