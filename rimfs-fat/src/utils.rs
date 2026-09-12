// SPDX-License-Identifier: MIT

//! FAT timestamp conversions and 8.3 short name generation.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, string::String, vec::Vec};

// SPDX-License-Identifier: MIT

use crate::{
    core::{
        errors::*,
        resolver::*,
        utils::{
            checksum_utils::checksum,
            time_utils::{self, TimeConversion},
        },
    },
    types::FatLFNEntry,
};

pub const MAX_LFN_CHARS: usize = 255;

/// Get datetime from attribute or fallback to now
pub fn datetime_from_attr(attr: &FileAttributes) -> (u16, u16, u8) {
    let ts = attr.modified.unwrap_or_else(time_utils::now_utc);
    ts.to_dos_datetime()
}

/// Allowed characters in SFN (after uppercase):
/// A–Z, 0–9 and !$%'-_@~`^#&(){}.
#[inline(always)]
fn is_valid_sfn_char(b: u8) -> bool {
    matches!(b,
        b'A'..=b'Z' | b'0'..=b'9' |
        b'!' | b'$' | b'%' | b'\'' | b'-' | b'_' | b'@' | b'~' | b'`' |
        b'^' | b'#' | b'&' | b'(' | b')' | b'{' | b'}'
    )
}

/// Suggest a short 8.3 name from input, return (short_name, is_lfn)
pub fn to_short_name(name: &str) -> ([u8; 11], bool) {
    to_short_name_unique(name, |_| false).expect("An empty directory always has a free alias")
}

/// Generate a short 8.3 name from input with numeric tail (~1, ~2, etc.) collision avoidance.
/// Returns (short_name, is_lfn).
pub fn to_short_name_unique<F>(name: &str, is_taken: F) -> FsParsingResult<([u8; 11], bool)>
where
    F: Fn(&[u8; 11]) -> bool,
{
    let parts: Vec<&str> = name.rsplitn(2, '.').collect();
    let (base, ext) = if parts.len() == 2 {
        (parts[1], parts[0])
    } else {
        (name, "")
    };

    let base_bytes = base.as_bytes();
    let ext_bytes = ext.as_bytes();

    let too_long = base_bytes.len() > 8 || ext_bytes.len() > 3;
    let has_space = base_bytes.contains(&b' ') || ext_bytes.contains(&b' ');
    let has_non_ascii =
        base.chars().any(|c| c as u32 > 0x7F) || ext.chars().any(|c| c as u32 > 0x7F);

    let mut clean_base: Vec<u8> = Vec::with_capacity(base_bytes.len());
    let mut base_ok = true;
    for &ch in base_bytes {
        if ch == b'.' || ch == b' ' {
            base_ok = false;
            continue;
        }
        let up = ch.to_ascii_uppercase();
        if is_valid_sfn_char(up) {
            clean_base.push(up);
        } else {
            base_ok = false;
            clean_base.push(b'_');
        }
    }

    let mut clean_ext: Vec<u8> = Vec::with_capacity(ext_bytes.len());
    let mut ext_ok = true;
    for &ch in ext_bytes {
        if ch == b' ' {
            ext_ok = false;
            continue;
        }
        let up = ch.to_ascii_uppercase();
        if is_valid_sfn_char(up) {
            clean_ext.push(up);
        } else {
            ext_ok = false;
            clean_ext.push(b'_');
        }
    }

    let all_spaces = clean_base.is_empty();
    let needs_lfn = too_long || has_space || has_non_ascii || !base_ok || !ext_ok || all_spaces;

    if !needs_lfn {
        let mut raw = [b' '; 11];
        for (i, &b) in clean_base.iter().take(8).enumerate() {
            raw[i] = b;
        }
        for (i, &b) in clean_ext.iter().take(3).enumerate() {
            raw[8 + i] = b;
        }
        if raw[0] == 0xE5 {
            raw[0] = 0x05;
        }
        if !is_taken(&raw) {
            return Ok((raw, name.bytes().any(|b| b.is_ascii_uppercase())));
        }
    }

    // Generate numeric tail (~1, ~2, ...)
    let mut ext_part = [b' '; 3];
    for (i, &b) in clean_ext.iter().take(3).enumerate() {
        ext_part[i] = b;
    }

    // Try suffixes ~1 through ~9999
    for i in 1..=9999 {
        let mut raw = [b' '; 11];
        let suffix = format!("~{}", i);
        let suffix_bytes = suffix.as_bytes();
        let max_base_len = 8usize.saturating_sub(suffix_bytes.len());
        let base_len = clean_base.len().min(max_base_len);

        for (j, &b) in clean_base.iter().take(base_len).enumerate() {
            raw[j] = b;
        }
        for (j, &b) in suffix_bytes.iter().enumerate() {
            raw[base_len + j] = b;
        }
        raw[8..11].copy_from_slice(&ext_part);

        if raw[0] == 0xE5 {
            raw[0] = 0x05;
        }

        if !is_taken(&raw) {
            return Ok((raw, true));
        }
    }

    Err(FsParsingError::Invalid(
        "Short-name alias namespace exhausted",
    ))
}

/// Decode SFN (8.3) entry to a filename
pub fn decode_sfn(sfn: &[u8; 11]) -> FsParsingResult<String> {
    let (name_raw, ext_raw) = sfn.split_at(8);

    let name = String::from_utf8(
        name_raw
            .iter()
            .take_while(|&&c| c != b' ')
            .map(|&c| c.to_ascii_lowercase())
            .collect(),
    )
    .map_err(|_| FsParsingError::Invalid("Invalid SFN"))?;

    let ext = String::from_utf8(
        ext_raw
            .iter()
            .take_while(|&&c| c != b' ')
            .map(|&c| c.to_ascii_lowercase())
            .collect(),
    )
    .map_err(|_| FsParsingError::Invalid("Invalid SFN"))?;

    if ext.is_empty() {
        Ok(name)
    } else {
        Ok(format!("{name}.{ext}"))
    }
}

/// Decode LFN entries into UTF-8 filename
pub fn decode_lfn(lfns: &[FatLFNEntry]) -> FsParsingResult<String> {
    if lfns.len() >= MAX_LFN_CHARS {
        return Err(FsParsingError::Invalid("LFN too long"));
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

    String::from_utf16(&name_utf16).map_err(|_| FsParsingError::Invalid("Invalid LFN"))
}

/// Generate a list of FatLFNEntry from name and short
pub fn lfn_entries(name: &str, short: &[u8; 11]) -> Vec<FatLFNEntry> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let count = name_utf16.len().div_ceil(13).max(1); // at least 1 entry
    let checksum: u8 = checksum(short);

    let mut out = Vec::with_capacity(count);

    for i in 0..count {
        let start = i * 13;
        let end = ((i + 1) * 13).min(name_utf16.len());
        let chunk = &name_utf16[start..end];

        let mut name_chars = [0xFFFFu16; 13];
        for (k, &cp) in chunk.iter().enumerate() {
            name_chars[k] = cp;
        }
        if end == name_utf16.len() && chunk.len() < 13 {
            name_chars[chunk.len()] = 0x0000;
        }

        // LFN order is 1..N; the *last* one on disk carries 0x40|N
        let order = (i + 1) as u8;
        let is_last = i + 1 == count;

        out.push(FatLFNEntry::new(order, is_last, &name_chars, checksum));
    }

    // On disk, we first write the entry with 0x40|N, then ..., then 0x01
    out.reverse();
    out
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn exhausted_aliases_are_errors() {
        assert!(to_short_name_unique("long_filename.txt", |_| true).is_err());
    }
}
