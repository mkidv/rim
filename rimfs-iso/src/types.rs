// SPDX-License-Identifier: MIT

//! ISO 9660 on-disk volume descriptor and directory record structures.

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::String;

use rimfs_core::allocator::FsHandle;
use time::OffsetDateTime;

/// Sector size in bytes for ISO 9660 optical disks.
pub const ISO_SECTOR_SIZE: usize = 2048;

/// Standard ISO 9660 volume identifier string.
pub const ISO_STANDARD_ID: &[u8; 5] = b"CD001";

/// Volume Descriptor type codes.
pub const VD_BOOT_RECORD: u8 = 0;
pub const VD_PRIMARY: u8 = 1;
pub const VD_SUPPLEMENTARY: u8 = 2;
pub const VD_PARTITION: u8 = 3;
pub const VD_TERMINATOR: u8 = 255;

/// El Torito Boot System Identifier (32 bytes).
pub const EL_TORITO_SYS_ID: &[u8; 32] = b"EL TORITO SPECIFICATION\0\0\0\0\0\0\0\0\0";

/// Joliet UCS-2 Level 3 escape sequence (%/@).
pub const JOLIET_ESCAPE_UCS2_LVL3: &[u8; 3] = b"%/@";

/// Directory record flags.
pub const DIR_FLAG_FILE: u8 = 0x00;
pub const DIR_FLAG_HIDDEN: u8 = 0x01;
pub const DIR_FLAG_DIRECTORY: u8 = 0x02;
pub const DIR_FLAG_ASSOCIATED: u8 = 0x04;
pub const DIR_FLAG_RECORD: u8 = 0x08;
pub const DIR_FLAG_PROTECTION: u8 = 0x10;
pub const DIR_FLAG_MULTIEXTENT: u8 = 0x80;

/// Handle representing an offset / LBA in an ISO image.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct IsoHandle(pub u64);
impl FsHandle for IsoHandle {}

impl From<u64> for IsoHandle {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

pub use crate::records::*;
use zerocopy::{FromBytes, IntoBytes};

/// Write the ISO 733 encoding into an existing composite buffer.
pub fn put_both_u32(buf: &mut [u8], val: u32) {
    buf[..8].copy_from_slice(BothE32::from(val).as_bytes());
}
/// Read and validate both copies of an ISO 733 value.
pub fn get_both_u32(buf: &[u8]) -> rimio::RimIOResult<u32> {
    BothE32::ref_from_prefix(buf)
        .map_err(|_| rimio::RimIOError::Invalid("Truncated ISO integer"))?
        .0
        .get()
}
/// Write the ISO 723 encoding into an existing composite buffer.
pub fn put_both_u16(buf: &mut [u8], val: u16) {
    buf[..4].copy_from_slice(BothE16::from(val).as_bytes());
}
/// Read and validate both copies of an ISO 723 value.
pub fn get_both_u16(buf: &[u8]) -> rimio::RimIOResult<u16> {
    BothE16::ref_from_prefix(buf)
        .map_err(|_| rimio::RimIOError::Invalid("Truncated ISO integer"))?
        .0
        .get()
}

/// Formats a 17-byte text date and time according to ISO 9660 Section 8.4.26.1 (YYYYMMDDHHMMSS00 + offset).
pub fn format_iso_text_datetime(dt: OffsetDateTime) -> [u8; 17] {
    let mut out = [b'0'; 17];
    let year = dt.year().clamp(0, 9999) as u32;
    let month = dt.month() as u8;
    let day = dt.day();
    let hour = dt.hour();
    let min = dt.minute();
    let sec = dt.second();

    let s = alloc::format!(
        "{:04}{:02}{:02}{:02}{:02}{:02}00",
        year,
        month,
        day,
        hour,
        min,
        sec
    );
    let bytes = s.as_bytes();
    let len = bytes.len().min(16);
    out[..len].copy_from_slice(&bytes[..len]);
    out[16] = 0; // GMT / UTC offset in 15-minute intervals
    out
}

/// Formats a 7-byte binary date and time according to ISO 9660 Section 9.1.5.
pub fn format_iso_binary_datetime(dt: OffsetDateTime) -> [u8; 7] {
    let year = dt.year().max(1900) - 1900;
    [
        year.min(255) as u8,
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        0, // GMT offset
    ]
}

/// Parses a 7-byte binary date and time from an ISO directory record.
pub fn parse_iso_binary_datetime(buf: &[u8; 7]) -> Option<OffsetDateTime> {
    let year = 1900 + buf[0] as i32;
    let month_val = buf[1];
    let day = buf[2];
    let hour = buf[3];
    let min = buf[4];
    let sec = buf[5];

    let month = time::Month::try_from(month_val).ok()?;
    let date = time::Date::from_calendar_date(year, month, day).ok()?;
    let time_obj = time::Time::from_hms(hour, min, sec).ok()?;
    Some(OffsetDateTime::new_utc(date, time_obj))
}

/// Encodes an ASCII/Latin-1 string into UCS-2 Big-Endian for Joliet SVD and directory records.
#[cfg(feature = "alloc")]
pub fn encode_ucs2_be(s: &str) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::with_capacity(s.len() * 2);
    for c in s.encode_utf16() {
        out.extend_from_slice(&c.to_be_bytes());
    }
    out
}

/// Decodes UCS-2 Big-Endian bytes into a UTF-8 String for Joliet file names.
#[cfg(feature = "alloc")]
pub fn decode_ucs2_be(bytes: &[u8]) -> String {
    let mut u16_chars = alloc::vec::Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let code = u16::from_be_bytes([chunk[0], chunk[1]]);
        if code == 0 {
            break;
        }
        u16_chars.push(code);
    }
    String::from_utf16_lossy(&u16_chars)
}
