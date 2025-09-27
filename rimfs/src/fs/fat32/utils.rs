// SPDX-License-Identifier: MIT

use crate::{
    core::{cursor::ClusterMeta, errors::*, resolver::*, utils::time_utils},
    fat32::Fat32Meta,
    fs::fat32::{
        constant::{FAT_ENTRY_SIZE, FAT_EOC},
        types::Fat32LFNEntry,
    },
};
use rimio::prelude::*;
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
    let ts = attr.modified.unwrap_or_else(time_utils::now_utc);
    datetime_from(ts)
}

pub fn datetime_now() -> (u16, u16, u8) {
    let ts = time_utils::now_utc();
    datetime_from(ts)
}

/// Compute FAT32 LFN checksum from short name
pub fn lfn_checksum(short: &[u8]) -> u8 {
    debug_assert_eq!(short.len(), 11);
    let mut sum = 0u8;
    for &b in short {
        sum = sum.rotate_right(1).wrapping_add(b);
    }
    sum
}

/// Caractères autorisés en SFN (après uppercase) :
/// A–Z, 0–9 et !$%'-_@~`^#&(){}.
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
    let mut raw = [b' '; 11];
    let mut is_lfn = false;

    // Split extension (à droite)
    let parts: Vec<&str> = name.rsplitn(2, '.').collect();
    let (base, ext) = if parts.len() == 2 {
        (parts[1], parts[0])
    } else {
        (name, "")
    };

    // Conditions LFN “strictes”
    // - longueur > 8/3
    // - présence d’espace
    // - présence de char non-ASCII
    // - présence de char ASCII interdit en SFN
    let base_bytes = base.as_bytes();
    let ext_bytes = ext.as_bytes();

    let too_long = base_bytes.len() > 8 || ext_bytes.len() > 3;
    let has_space = base_bytes.iter().any(|&c| c == b' ') || ext_bytes.iter().any(|&c| c == b' ');
    let has_non_ascii =
        base.chars().any(|c| c as u32 > 0x7F) || ext.chars().any(|c| c as u32 > 0x7F);

    // On upper-case *ASCII* et on remplace les invalides par '_'
    let mut base_ok = true;
    for (i, ch) in base_bytes.iter().take(8).enumerate() {
        let up = ch.to_ascii_uppercase();
        let out = if is_valid_sfn_char(up) {
            up
        } else {
            base_ok = false;
            b'_'
        };
        raw[i] = out;
    }
    let mut ext_ok = true;
    for (i, ch) in ext_bytes.iter().take(3).enumerate() {
        let up = ch.to_ascii_uppercase();
        let out = if is_valid_sfn_char(up) {
            up
        } else {
            ext_ok = false;
            b'_'
        };
        raw[8 + i] = out;
    }

    // Si tout était vide → pas de nom valable en SFN
    let all_spaces = !raw.iter().any(|&b| b != b' ');
    // LFN requis si l’une des conditions suivantes est vraie
    is_lfn = too_long || has_space || has_non_ascii || !base_ok || !ext_ok || all_spaces;

    // Règle 0xE5 => 0x05 (si 1er octet de SFN vaut 0xE5)
    if raw[0] == 0xE5 {
        raw[0] = 0x05;
    }

    (raw, is_lfn)
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
pub fn decode_lfn(lfns: &[Fat32LFNEntry]) -> FsParsingResult<String> {
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

/// Generate a list of Fat32LFNEntry from name and short
pub fn lfn_entries(name: &str, short: &[u8; 11]) -> Vec<Fat32LFNEntry> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let count = name_utf16.len().div_ceil(13).max(1); // au moins 1 entrée
    let checksum = lfn_checksum(short);

    let mut out = Vec::with_capacity(count);

    for i in 0..count {
        let start = i * 13;
        let end = ((i + 1) * 13).min(name_utf16.len());
        let chunk = &name_utf16[start..end];

        // Prépare un buffer 13 UTF-16 = [0xFFFF...], et place le terminator 0x0000 si place
        let mut name_chars = [0xFFFFu16; 13];
        for (k, &cp) in chunk.iter().enumerate() {
            name_chars[k] = cp;
        }
        if end == name_utf16.len() && chunk.len() < 13 {
            name_chars[chunk.len()] = 0x0000;
        }

        // L’ordre LFN est 1..N ; le *dernier* sur disque porte 0x40|N
        let order = (i + 1) as u8;
        let is_last = i + 1 == count;

        out.push(Fat32LFNEntry::new(order, is_last, &name_chars, checksum));
    }

    // Sur le disque, on écrit d'abord l'entrée avec 0x40|N, puis …, puis 0x01
    out.reverse();
    out
}
