// SPDX-License-Identifier: MIT

//! Time utilities for filesystem timestamps.
//!
//! Provides UTC time handling with `no_std` fallback support.
//!
//! - In `std` mode, uses system clock.
//! - In `no_std`, returns UNIX_EPOCH as fixed timestamp.
//!
//! Functions:
//! - `now_utc()` → current UTC time
//! - `utc_offset()` → current UTC offset
//! - `systemtime_to_offsetdatetime()` → conversion helper (std only)

#[cfg(feature = "std")]
use std::time::SystemTime;

use time::{OffsetDateTime, UtcOffset};

/// Converts a [`SystemTime`] into an [`OffsetDateTime`].
///
/// Only available in `std` mode.
#[cfg(feature = "std")]
pub fn systemtime_to_offsetdatetime(t: SystemTime) -> OffsetDateTime {
    OffsetDateTime::from(t)
}

/// Returns the current UTC time.
///
/// - In `std` mode, returns the actual system UTC time.
/// - In `no_std`, returns `OffsetDateTime::UNIX_EPOCH` as fallback.
pub fn now_utc() -> OffsetDateTime {
    #[cfg(feature = "std")]
    {
        OffsetDateTime::now_utc()
    }

    #[cfg(not(feature = "std"))]
    {
        // Fallback: use UNIX_EPOCH (1970-01-01T00:00:00Z).
        OffsetDateTime::UNIX_EPOCH
    }
}

/// Returns the current UTC offset.
///
/// - In `std` mode, uses the system clock offset.
/// - In `no_std`, returns `UtcOffset::UTC` as fallback.
pub fn utc_offset() -> UtcOffset {
    #[cfg(feature = "std")]
    {
        // This will use the system clock UTC offset if available.
        OffsetDateTime::now_utc().offset()
    }

    #[cfg(not(feature = "std"))]
    {
        UtcOffset::UTC
    }
}

/// Trait for converting [`OffsetDateTime`] to filesystem-specific formats.
pub trait TimeConversion {
    /// Convert to DOS date and time (Date, Time, Tenths)
    /// Used by FAT and exFAT.
    fn to_dos_datetime(&self) -> (u16, u16, u8);

    /// Convert to NTFS FILETIME (100-ns intervals since 1601-01-01)
    fn to_ntfs_filetime(&self) -> u64;

    /// Convert to Unix timestamp as u32 (seconds since 1970-01-01)
    /// Used by Ext2/3/4.
    fn to_unix_u32(&self) -> u32;

    /// Convert to exFAT format (Date|Time packed, 10ms increment, UTC offset)
    fn to_exfat_datetime(&self) -> (u32, u8, u8);
}

impl TimeConversion for OffsetDateTime {
    fn to_dos_datetime(&self) -> (u16, u16, u8) {
        let year = self.year().clamp(1980, 2107);
        let month = self.month() as u16;
        let day = self.day() as u16;

        let hour = self.hour() as u16;
        let minute = self.minute() as u16;
        let second = self.second() as u16;

        let subsec = self.millisecond() / 10;

        let date = ((year - 1980) as u16) << 9 | (month << 5) | day;
        let time = (hour << 11) | (minute << 5) | (second / 2);

        (date, time, subsec as u8)
    }

    fn to_ntfs_filetime(&self) -> u64 {
        const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;
        let nanos = self.unix_timestamp_nanos();
        let ticks = (nanos / 100) as u64;
        ticks + FILETIME_UNIX_DIFF
    }

    fn to_unix_u32(&self) -> u32 {
        self.unix_timestamp() as u32
    }

    fn to_exfat_datetime(&self) -> (u32, u8, u8) {
        let year = self.year().clamp(1980, 2107) as u32;
        let month = self.month() as u32;
        let day = self.day() as u32;
        let hour = self.hour() as u32;
        let minute = self.minute() as u32;
        let second = self.second() as u32;

        let date = ((year - 1980) << 25) | (month << 21) | (day << 16);
        let time = (hour << 11) | (minute << 5) | (second / 2);
        let encoded = date | time;

        let millis_10ms = (self.millisecond() / 10) as u8;

        let offset = self.offset().whole_minutes();
        let utc_offset_15min = (offset / 15).clamp(-64, 63);
        let utc_encoded = if utc_offset_15min < 0 {
            ((!(-utc_offset_15min) as u8) + 1) & 0x7F // two's complement
        } else {
            utc_offset_15min as u8
        };

        (encoded, millis_10ms, utc_encoded | 0x80) // Set bit 7 (OffsetValid)
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_now_utc_and_offset() {
        let now = now_utc();
        println!("Current UTC time: {now:?}");
        let offset = utc_offset();
        println!("Current UTC offset: {offset:?}");
    }

    #[test]
    fn test_systemtime_to_offsetdatetime() {
        use std::time::SystemTime;
        let st = SystemTime::now();
        let odt = systemtime_to_offsetdatetime(st);
        println!("Converted SystemTime to OffsetDateTime: {odt:?}");
    }

    #[test]
    fn test_time_conversion() {
        let epoch = OffsetDateTime::UNIX_EPOCH;

        // Unix
        assert_eq!(epoch.to_unix_u32(), 0);

        // NTFS: 1970-01-01 is 11644473600 seconds after 1601-01-01
        // 11644473600 * 10_000_000 ticks/sec = 116444736000000000
        assert_eq!(epoch.to_ntfs_filetime(), 116_444_736_000_000_000);

        // DOS (1980-01-01 minimum)
        // 1970 clamps to 1980
        let (date, time, tenth) = epoch.to_dos_datetime();
        // Date: (1980-1980)<<9 | 1<<5 | 1 = 0 | 32 | 1 = 33 (0x21)
        assert_eq!(date, 0x21);
        assert_eq!(time, 0);
        assert_eq!(tenth, 0);
    }
}
