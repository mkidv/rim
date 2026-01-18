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
}
