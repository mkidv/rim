// SPDX-License-Identifier: MIT

use crate::{core::{parser::*, utils::time_utils}, fs::fat32::types::Fat32LFNEntry};
use time::OffsetDateTime;

pub const MAX_LFN_CHARS: usize = 255;

/// Format datetime to FAT32 (date, time, fine resolution)
fn datetime_from(ts: OffsetDateTime) -> (u16, u16, u8) {
    let year = ts.year().clamp(1980, 2107);
    let month = ts.month() as u16;
    let day = ts.day() as u16;

    let hour = ts.hour() as u16;
    let minute = ts.minute() as u16;
    let second = ts.second() as u16;

    let subsec = ts.millisecond() / 10;

    let date = ((year - 1980) as u16) << 9 | (month << 5) | day;
    let time = (hour << 11) | (minute << 5) | (second / 2);

    (date, time, subsec as u8)
}

/// Get datetime from attribute or fallback to now
pub fn datetime_from_attr(attr: &FileAttributes) -> (u16, u16, u8) {
    let ts = attr
        .modified
        .unwrap_or_else(time_utils::now_utc);
    datetime_from(ts)
}

pub fn datetime_now()-> (u16, u16, u8) {
    let ts = time_utils::now_utc();
    datetime_from(ts)
}

/// Compute FAT32 LFN checksum from short name
pub fn lfn_checksum(short: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for &b in short {
        sum = ((sum & 1) << 7).wrapping_add(sum >> 1).wrapping_add(b);
    }
    sum
}

/// Suggest a short 8.3 name from input, return (short_name, is_lfn)
pub fn to_short_name(name: &str) -> ([u8; 11], bool) {
    let mut raw = [b' '; 11];
    let mut is_lfn = false;

    let parts: Vec<&str> = name.rsplitn(2, '.').collect();
    if parts.len() == 2 {
        let (ext, base) = (parts[0], parts[1]);
        if base.len() > 8 || ext.len() > 3 {
            is_lfn = true;
        }
        for (i, b) in base.bytes().take(8).enumerate() {
            raw[i] = b.to_ascii_uppercase();
        }
        for (i, b) in ext.bytes().take(3).enumerate() {
            raw[8 + i] = b.to_ascii_uppercase();
        }
    } else {
        for (i, b) in name.bytes().take(8).enumerate() {
            raw[i] = b.to_ascii_uppercase();
        }
        if name.len() > 8 {
            is_lfn = true;
        }
    }

    if !raw.iter().any(|&b| b != b' ') {
        is_lfn = true;
    }

    (raw, is_lfn)
}

/// Decode SFN (8.3) entry to a filename
pub fn decode_sfn(sfn: &[u8; 11]) -> FsParserResult<String> {
    let (name_raw, ext_raw) = sfn.split_at(8);

    let name = String::from_utf8(
        name_raw
            .iter()
            .take_while(|&&c| c != b' ')
            .map(|&c| c.to_ascii_lowercase())
            .collect(),
    )
    .map_err(|_| FsParserError::Invalid("Invalid SFN"))?;

    let ext = String::from_utf8(
        ext_raw
            .iter()
            .take_while(|&&c| c != b' ')
            .map(|&c| c.to_ascii_lowercase())
            .collect(),
    )
    .map_err(|_| FsParserError::Invalid("Invalid SFN"))?;

    if ext.is_empty() {
        Ok(name)
    } else {
        Ok(format!("{name}.{ext}"))
    }
}

/// Decode LFN entries into UTF-8 filename
pub fn decode_lfn(lfns: &[Fat32LFNEntry]) -> FsParserResult<String> {
    if lfns.len() >= MAX_LFN_CHARS {
        return Err(FsParserError::Invalid("LFN too long"));
    }

    let mut name_utf16 = Vec::with_capacity(MAX_LFN_CHARS);
    for entry in lfns.iter().rev() {
        for &c in &entry.extract_utf16() {
            if c == 0x0000 || c == 0xFFFF {
                break;
            }
            name_utf16.push(c);
        }
    }

    String::from_utf16(&name_utf16).map_err(|_| FsParserError::Invalid("Invalid LFN"))
}

/// Generate a list of Fat32LFNEntry from name and short
pub fn lfn_entries(name: &str, short: &[u8; 11]) -> Vec<crate::fs::fat32::types::Fat32LFNEntry> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let count = name_utf16.len().div_ceil(13);
    let checksum = lfn_checksum(short);

    (0..count)
        .rev()
        .map(|i| {
            let start = i * 13;
            let end = ((i + 1) * 13).min(name_utf16.len());
            crate::fs::fat32::types::Fat32LFNEntry::new(
                (i + 1) as u8,
                i == count - 1,
                &name_utf16[start..end],
                checksum,
            )
        })
        .collect()
}
