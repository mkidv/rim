use crate::{
    core::{parser::*, utils::time_utils},
    fs::exfat::{constant::*, meta::*, types::ExFatNameEntry},
};
use rimio::prelude::*;
use time::OffsetDateTime;

/// Encode ExFAT datetime (as `u32 + u8 + u8`):
/// - `u32` = date+time (same layout que FAT32)
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

pub fn name_entries(name: &str) -> Vec<ExFatNameEntry> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let count = name_utf16.len().div_ceil(15);

    (0..count)
        .map(|i| {
            let start = i * 15;
            let end = ((i + 1) * 15).min(name_utf16.len());

            let mut name_chars = [0x0000u16; 15];
            for (j, &c) in name_utf16[start..end].iter().enumerate() {
                name_chars[j] = c;
            }

            ExFatNameEntry::new(name_chars)
        })
        .collect()
}

pub fn decode_name(names: &[ExFatNameEntry]) -> FsParserResult<String> {
    let mut name_utf16 = Vec::with_capacity(names.len() * 15);
    for name_entry in names {
        let name_chars = name_entry.name_chars;
        for &c in name_chars.iter() {
            if c == 0x0000 || c == 0xFFFF {
                break;
            }
            name_utf16.push(c);
        }
    }
    String::from_utf16(&name_utf16)
        .map_err(|_| FsParserError::Invalid("Invalid UTF-16 in ExFat name"))
}

pub fn compute_name_hash(name: &str) -> u16 {
    name.encode_utf16().fold(0u16, |acc, c| {
        let rotated = acc.rotate_right(1);
        rotated.wrapping_add(c)
    })
}

pub fn write_fat_chain<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    clusters: &[u32],
) -> BlockIOResult {
    let mut fat_entries = Vec::with_capacity(clusters.len() * EXFAT_ENTRY_SIZE);

    for (i, _) in clusters.iter().enumerate() {
        let next_cluster = if i + 1 < clusters.len() {
            clusters[i + 1]
        } else {
            0xFFFFFFFF
        };
        fat_entries.extend_from_slice(&next_cluster.to_le_bytes());
    }

    let offsets: Vec<u64> = clusters
        .iter()
        .map(|&cluster| meta.fat_entry_offset(cluster))
        .collect();

    io.write_multi_at(&offsets, EXFAT_ENTRY_SIZE, &fat_entries)?;
    Ok(())
}

pub fn write_bitmap<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    clusters: &[u32],
) -> BlockIOResult {
    let offset = meta.unit_offset(meta.bitmap_cluster);
    let mut bitmap = vec![0u8; meta.unit_size()];
    io.read_block_best_effort(offset, &mut bitmap, meta.unit_size())?;

    for &cluster in clusters {
        let (byte_index, bit_mask) = meta.bitmap_entry_offset(cluster);
        bitmap[byte_index] |= bit_mask;
    }

    io.write_block_best_effort(offset, &bitmap, meta.unit_size())?;
    Ok(())
}

pub fn generate_minimal_upcase_table() -> Vec<u8> {
    let mut table = Vec::with_capacity(512);
    for c in 0u8..=0xFF {
        let upper = c.to_ascii_uppercase() as u16;
        table.extend_from_slice(&upper.to_le_bytes());
    }
    while table.len() < 512 {
        table.extend_from_slice(&0xFFFFu16.to_le_bytes());
    }
    table
}

pub fn generate_full_upcase_table() -> Vec<u8> {
    let mut table = Vec::with_capacity(5836);
    for c in 0u16..=0xFFFF {
        let upper = to_unicode_upper_or_ff(c);
        table.extend_from_slice(&upper.to_le_bytes());
    }
    table
}

/// Returns the upper-case of a codepoint if available, or 0xFFFF if invalid or unmapped.
pub fn to_unicode_upper_or_ff(c: u16) -> u16 {
    match c {
        // Basic Latin (a-z → A-Z)
        0x0061..=0x007A => c - 0x20,

        // Latin-1 Supplement (à-ö, ø-ÿ → À-Ö, Ø-ß)
        0x00E0..=0x00F6 => c - 0x20,
        0x00F8..=0x00FE => c - 0x20,

        // Latin Extended-A
        0x0100..=0x017F if c % 2 == 1 => c - 1,
        0x0100..=0x017F if c % 2 == 0 => c,

        // Greek (α-ω → Α-Ω)
        0x03B1..=0x03C1 => c - 0x20,
        0x03C3..=0x03CB => c - 0x20,

        // Cyrillic (а-я → А-Я)
        0x0430..=0x044F => c - 0x20,

        // Extended Latin and known 1:1 mappings
        0x2170..=0x217F => c - 0x10, // small Roman numerals → capital
        0x24D0..=0x24E9 => c - 0x1A, // enclosed a-z → A-Z

        // Control / surrogate / private use = invalid
        0xD800..=0xDFFF => 0xFFFF,
        0xF000..=0xFFFF => 0xFFFF,

        // All other: identity (no change)
        _ => c,
    }
}
