// SPDX-License-Identifier: MIT

//! exFAT timestamp decoding and path conversion utilities.

use crate::core::{
    resolver::*,
    utils::time_utils::{self, TimeConversion},
};

use time::OffsetDateTime;

/// Get datetime from attribute or fallback to now
pub fn datetime_from_attr(attr: &FileAttributes) -> (u32, u8, u8) {
    let ts = attr.modified.unwrap_or_else(time_utils::now_utc);
    ts.to_exfat_datetime()
}

/// Converts exFAT packed date/time, 10ms increment, and UTC offset byte into an [`OffsetDateTime`].
pub fn exfat_to_datetime(
    packed: u32,
    fine_10ms: u8,
    utc_offset_byte: u8,
) -> Option<OffsetDateTime> {
    if packed == 0 {
        return None;
    }

    let year = 1980 + ((packed >> 25) & 0x7F) as i32;
    let month_val = ((packed >> 21) & 0x0F) as u8;
    let day = ((packed >> 16) & 0x1F) as u8;
    let hour = ((packed >> 11) & 0x1F) as u8;
    let minute = ((packed >> 5) & 0x3F) as u8;
    let sec_double = (packed & 0x1F) as u8;

    let base_sec = sec_double * 2;
    if fine_10ms >= 200 {
        return None;
    }
    let second = base_sec + fine_10ms / 100;
    let milli = (fine_10ms % 100) as u16 * 10;

    let offset = if utc_offset_byte & 0x80 != 0 {
        let raw7 = (utc_offset_byte & 0x7F) as i16;
        let intervals_15min = if raw7 >= 64 { raw7 - 128 } else { raw7 };
        let offset_seconds = (intervals_15min as i32) * 15 * 60;
        time::UtcOffset::from_whole_seconds(offset_seconds).ok()?
    } else {
        time::UtcOffset::UTC
    };

    let month = time::Month::try_from(month_val).ok()?;
    let date_obj = time::Date::from_calendar_date(year, month, day).ok()?;
    let time_obj = time::Time::from_hms_milli(hour, minute, second, milli).ok()?;
    Some(date_obj.with_time(time_obj).assume_offset(offset))
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_exfat_to_datetime_roundtrip() {
        let offset = time::UtcOffset::from_hms(2, 0, 0).unwrap();
        let date = time::Date::from_calendar_date(2024, time::Month::August, 20).unwrap();
        let time = time::Time::from_hms_milli(16, 45, 30, 250).unwrap();
        let original = date.with_time(time).assume_offset(offset);

        let (packed, fine_10ms, utc_offset_byte) = original.to_exfat_datetime();
        let decoded = exfat_to_datetime(packed, fine_10ms, utc_offset_byte).unwrap();

        assert_eq!(decoded.year(), 2024);
        assert_eq!(decoded.month(), time::Month::August);
        assert_eq!(decoded.day(), 20);
        assert_eq!(decoded.hour(), 16);
        assert_eq!(decoded.minute(), 45);
        assert_eq!(decoded.second(), 30);
        assert_eq!(decoded.millisecond(), 250);
        assert_eq!(decoded.offset(), offset);

        // Invalid packed value
        assert!(exfat_to_datetime(0, 0, 0).is_none());
    }
}

#[cfg(test)]
mod timestamp_boundaries {
    use super::*;
    #[test]
    fn preserves_second_parity_and_rejects_invalid_fields() {
        let date = time::Date::from_calendar_date(2024, time::Month::August, 20).unwrap();
        for second in 0..60 {
            for millis in [0, 10, 250, 990] {
                for minutes in [-720, -345, 0, 330, 840] {
                    let offset = time::UtcOffset::from_whole_seconds(minutes * 60).unwrap();
                    let source = date
                        .with_hms_milli(16, 45, second, millis)
                        .unwrap()
                        .assume_offset(offset);
                    let (packed, fine, utc) = source.to_exfat_datetime();
                    assert_eq!(exfat_to_datetime(packed, fine, utc), Some(source));
                    assert!(exfat_to_datetime(packed, 200, utc).is_none());
                    assert!(exfat_to_datetime((packed & !31) | 31, 0, utc).is_none());
                }
            }
        }
    }
}
