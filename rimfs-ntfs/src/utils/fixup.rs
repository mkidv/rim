// SPDX-License-Identifier: MIT
//! Update Sequence Array (USA) fixup utilities.
//!
//! NTFS uses USA fixups on multi-sector blocks (MFT records, INDX records) to detect
//! torn writes. The last 2 bytes of each 512-byte sector are moved into the USA array
//! in the record header, and replaced by a sequence check value.

/// Apply Update Sequence Array (USA) fixups to a record (Write-side)
///
/// This replaces the last two bytes of each sector with a check value
/// and stores the original bytes in the USA array.
pub fn apply_usa_fixup(record: &mut [u8], sector_size: usize) {
    if record.len() < 48 {
        return;
    }

    let usa_offset = u16::from_le_bytes([record[4], record[5]]) as usize;
    let usa_count = u16::from_le_bytes([record[6], record[7]]) as usize;

    if usa_count < 2 || usa_offset + usa_count * 2 > record.len() {
        return;
    }

    let check_value = u16::from_le_bytes([record[usa_offset], record[usa_offset + 1]]);

    for i in 1..usa_count {
        let sector_end = i * sector_size - 2;
        if sector_end + 1 < record.len() {
            // Save original bytes to USA
            record[usa_offset + i * 2] = record[sector_end];
            record[usa_offset + i * 2 + 1] = record[sector_end + 1];

            // Replace in sector with check value
            let check_bytes = check_value.to_le_bytes();
            record[sector_end] = check_bytes[0];
            record[sector_end + 1] = check_bytes[1];
        }
    }
}

/// Remove Update Sequence Array (USA) fixups from a record (Read-side)
///
/// This restores the original bytes to the end of each sector from the USA array
/// after verifying the check value.
pub fn decode_usa_fixup(record: &mut [u8], sector_size: usize) -> bool {
    if record.len() < 48 {
        return false;
    }

    let usa_offset = u16::from_le_bytes([record[4], record[5]]) as usize;
    let usa_count = u16::from_le_bytes([record[6], record[7]]) as usize;

    if usa_count < 2 || usa_offset + usa_count * 2 > record.len() {
        return false;
    }

    let check_value = u16::from_le_bytes([record[usa_offset], record[usa_offset + 1]]);

    for i in 1..usa_count {
        let sector_end = i * sector_size - 2;
        if sector_end + 1 < record.len() {
            let found = u16::from_le_bytes([record[sector_end], record[sector_end + 1]]);
            if found != check_value {
                return false; // Corruption detected
            }

            // Restore original bytes
            record[sector_end] = record[usa_offset + i * 2];
            record[sector_end + 1] = record[usa_offset + i * 2 + 1];
        }
    }
    true
}

/// Calculate the number of USA elements needed for a record
pub const fn calculate_usa_size(record_size: u32, sector_size: u32) -> u16 {
    // Number of sectors in the record + 1 (for the check value)
    ((record_size / sector_size) + 1) as u16
}
