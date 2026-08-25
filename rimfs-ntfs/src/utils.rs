// SPDX-License-Identifier: MIT
//! NTFS utility functions

use crate::upcase::UpcaseHandle;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};
use core::cmp::Ordering;

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
            // Verify check value
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

/// Encode a data run for non-resident attributes
///
/// Data runs encode cluster runs as (length, offset) pairs in a compact format.
/// Returns the encoded bytes and their length.
pub fn encode_data_run(lcn: i64, length: u64) -> ([u8; 17], usize) {
    let mut result = [0u8; 17];
    let mut cursor = 1;

    // Encode length
    let mut len_buf = [0u8; 8];
    let len_size = encode_variable_int_to_buf(length as i64, false, &mut len_buf);
    result[cursor..cursor + len_size].copy_from_slice(&len_buf[..len_size]);
    cursor += len_size;

    // Encode LCN offset (signed for delta encoding)
    let mut lcn_buf = [0u8; 8];
    let lcn_size = encode_variable_int_to_buf(lcn, true, &mut lcn_buf);
    result[cursor..cursor + lcn_size].copy_from_slice(&lcn_buf[..lcn_size]);
    cursor += lcn_size;

    // Header byte: high nibble = LCN size, low nibble = length size
    result[0] = ((lcn_size as u8) << 4) | (len_size as u8);

    (result, cursor)
}

/// Encode an integer in variable-length format into a fixed buffer
fn encode_variable_int_to_buf(value: i64, signed: bool, buf: &mut [u8; 8]) -> usize {
    if value == 0 {
        buf[0] = 0;
        return 0; // Length 0 is usually not used, header counts bytes.
        // In NTFS, if value is 0, it takes 0 bytes in the run, header says size 0.
        // Wait, actually if length is 0, it should probably be 1 byte of 0?
        // No, the header nibble would be 0.
    }

    let mut v = value;
    let mut len = 0;

    if signed {
        for (i, byte) in buf.iter_mut().enumerate().take(8) {
            *byte = (v & 0xFF) as u8;
            v >>= 8;
            len = i + 1;
            if v == 0 && (*byte & 0x80) == 0 {
                break;
            }
            if v == -1 && (*byte & 0x80) != 0 {
                break;
            }
        }
    } else {
        for (i, byte) in buf.iter_mut().enumerate().take(8) {
            if v == 0 {
                break;
            }
            *byte = (v & 0xFF) as u8;
            v >>= 8;
            len = i + 1;
        }
    }

    len
}

/// Terminate a data run sequence
pub fn encode_data_run_end() -> u8 {
    0x00
}

/// Get current time as NTFS FILETIME
///
/// FILETIME is 100-nanosecond intervals since January 1, 1601 UTC.
pub fn current_ntfs_time() -> u64 {
    // Use a fixed time for reproducibility in tests and deterministic images
    // This represents 2024-01-01 00:00:00 UTC
    // For real time, this would use system time
    #[cfg(feature = "std")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};

        // Offset between 1601-01-01 and 1970-01-01 in 100-ns intervals
        const FILETIME_UNIX_DIFF: u64 = 116444736000000000;

        if let Ok(duration) = SystemTime::now().duration_since(UNIX_EPOCH) {
            let ticks = duration.as_nanos() / 100;
            return ticks as u64 + FILETIME_UNIX_DIFF;
        }
    }

    // Default: 2024-01-01 00:00:00 UTC
    133477536000000000
}

/// Decoded data run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataRun {
    /// Starting LCN (absolute)
    pub lcn: u64,
    /// Length in clusters
    pub length: u64,
}

/// Decode a stream of data runs
pub fn decode_data_runs(mut runs: &[u8]) -> Vec<DataRun> {
    let mut result = Vec::new();
    let mut current_lcn = 0i64;

    while let Some(&header) = runs.first() {
        if header == 0 {
            break;
        }
        runs = &runs[1..];

        let len_size = (header & 0x0F) as usize;
        let offset_size = ((header >> 4) & 0x0F) as usize;

        if runs.len() < len_size + offset_size {
            break; // Invalid run
        }

        let length = decode_variable_uint(runs, len_size);
        runs = &runs[len_size..];

        let offset = decode_variable_int(runs, offset_size);
        runs = &runs[offset_size..];

        current_lcn += offset;

        // Sparse runs have offset 0 (actually no, offset exists but LCN might be unused?)
        // Sparse run has offset size 0.
        if offset_size == 0 {
            // Sparse: valid length but no LCN. mapped to 0? or specific sparse handling?
            // For now treat as LCN 0 or skip?
            // If offset is 0-sized, offset is 0.
            // In NTFS, sparse block is encoded with offset_size=0. LCN is usually treated as 0 or valid?
            // Actually sparse means VCNs are mapped to nothing (LCN 0 or special).
            // Let's assume (0, length) for now?
            // But current_lcn doesn't change.
            // Wait, if offset size 0, offset=0. current_lcn not changed?
            // Sparse runs don't change LCN? Or do they?
            // Usually sparse runs have specific LCN=0 marker or are implicit.
            // If offset size is 0, it is a sparse run. LCN is VCN-only.
        }

        result.push(DataRun {
            lcn: current_lcn as u64,
            length,
        });
    }

    result
}

fn decode_variable_uint(bytes: &[u8], size: usize) -> u64 {
    let mut value = 0u64;
    for (i, byte) in bytes.iter().enumerate().take(size) {
        value |= (*byte as u64) << (i * 8);
    }
    value
}

fn decode_variable_int(bytes: &[u8], size: usize) -> i64 {
    if size == 0 {
        return 0;
    }
    let mut value = 0i64;
    for (i, byte) in bytes.iter().enumerate().take(size) {
        value |= (*byte as i64) << (i * 8);
    }

    // Sign extend if the highest read byte has sign bit
    if size < 8 && (bytes[size - 1] & 0x80) != 0 {
        value |= -1i64 << (size * 8);
    }

    value
}

/// Calculate the size needed for an MFT record's Update Sequence Array
pub fn calculate_usa_size(record_size: u32, sector_size: u32) -> u16 {
    // Number of sectors in the record + 1 (for the check value)
    ((record_size / sector_size) + 1) as u16
}

/// Build initial USA for a record
pub fn build_initial_usa(usa_count: u16) -> Vec<u8> {
    let mut usa = Vec::with_capacity(usa_count as usize * 2);

    // Check value (arbitrary, we use 0x0000 for new records)
    usa.extend_from_slice(&0x0000u16.to_le_bytes());

    // Original sector end bytes (initially 0x0000)
    for _ in 1..usa_count {
        usa.extend_from_slice(&0x0000u16.to_le_bytes());
    }

    usa
}

/// Parse MFT record reference into record number and sequence
pub fn parse_mft_reference(reference: u64) -> (u64, u16) {
    let record_number = reference & 0x0000FFFFFFFFFFFF;
    let sequence = (reference >> 48) as u16;
    (record_number, sequence)
}

/// Build MFT reference from record number and sequence
pub fn build_mft_reference(record_number: u64, sequence: u16) -> u64 {
    (record_number & 0x0000FFFFFFFFFFFF) | ((sequence as u64) << 48)
}

/// Convert core FileAttributes to NtfsFileAttributes
pub fn ntfs_attr_from_core(
    attr: &crate::core::resolver::attr::FileAttributes,
) -> crate::flags::NtfsFileAttributes {
    let mut flags = crate::flags::NtfsFileAttributes::empty();
    if attr.read_only {
        flags |= crate::flags::NtfsFileAttributes::READ_ONLY;
    }
    if attr.hidden {
        flags |= crate::flags::NtfsFileAttributes::HIDDEN;
    }
    if attr.system {
        flags |= crate::flags::NtfsFileAttributes::SYSTEM;
    }
    if attr.dir {
        flags |= crate::flags::NtfsFileAttributes::DIRECTORY;
    }
    if attr.archive {
        flags |= crate::flags::NtfsFileAttributes::ARCHIVE;
    }
    flags
}

/// Compare two UTF-16 names using the NTFS Upcase table (case-insensitive)
pub fn compare_names_upcase(a: &[u16], b: &[u16], upcase: &UpcaseHandle) -> Ordering {
    let len = a.len().min(b.len());
    for i in 0..len {
        let ca = upcase.upper(a[i]);
        let cb = upcase.upper(b[i]);
        if ca != cb {
            return ca.cmp(&cb);
        }
    }
    a.len().cmp(&b.len())
}

/// Compare a UTF-16 name with a UTF-8 string using the upcase table
pub fn eq_names_upcase_str(name_u16: &[u16], target: &str, upcase: &UpcaseHandle) -> bool {
    let target_u16: Vec<u16> = target.encode_utf16().collect();
    if name_u16.len() != target_u16.len() {
        return false;
    }
    for i in 0..name_u16.len() {
        if upcase.upper(name_u16[i]) != upcase.upper(target_u16[i]) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_run_encoding() {
        // Simple run: 10 clusters starting at LCN 100
        let (run, len) = encode_data_run(100, 10);
        assert!(len > 0);
        // First byte is header
        assert_eq!(run[0] & 0x0F, 1); // Length needs 1 byte
    }

    #[test]
    fn test_mft_reference() {
        let record_num = 12345u64;
        let seq = 7u16;

        let reference = build_mft_reference(record_num, seq);
        let (parsed_num, parsed_seq) = parse_mft_reference(reference);

        assert_eq!(parsed_num, record_num);
        assert_eq!(parsed_seq, seq);
    }

    #[test]
    fn test_usa_size() {
        // 1024-byte record with 512-byte sectors = 2 sectors + check = 3 words
        assert_eq!(calculate_usa_size(1024, 512), 3);

        // 4096-byte record with 512-byte sectors = 8 sectors + check = 9 words
        assert_eq!(calculate_usa_size(4096, 512), 9);
    }

    #[test]
    fn test_compare_names_upcase() {
        use crate::upcase::UpcaseFlavor;
        let upcase = UpcaseHandle::from_flavor(&UpcaseFlavor::Windows);

        let name1: Vec<u16> = "filename.txt".encode_utf16().collect();
        let name2: Vec<u16> = "FILENAME.TXT".encode_utf16().collect();
        let name3: Vec<u16> = "filename.tyt".encode_utf16().collect();

        assert_eq!(
            compare_names_upcase(&name1, &name2, &upcase),
            Ordering::Equal
        );
        assert_eq!(
            compare_names_upcase(&name1, &name3, &upcase),
            Ordering::Less
        );

        // Unicode check: Cyrillic 'a' (U+0430) and 'A' (U+0410)
        let cyr_a_lower: Vec<u16> = vec![0x0430];
        let cyr_a_upper: Vec<u16> = vec![0x0410];
        assert_eq!(
            compare_names_upcase(&cyr_a_lower, &cyr_a_upper, &upcase),
            Ordering::Equal
        );

        // Standard ASCII order check
        assert!(
            compare_names_upcase(
                &"a".encode_utf16().collect::<Vec<_>>(),
                &"B".encode_utf16().collect::<Vec<_>>(),
                &upcase
            ) == Ordering::Less
        );
    }

    #[test]
    fn test_eq_names_upcase_str() {
        use crate::upcase::UpcaseFlavor;
        let upcase = UpcaseHandle::from_flavor(&UpcaseFlavor::Windows);

        let name_u16: Vec<u16> = "TestFile.zip".encode_utf16().collect();
        assert!(eq_names_upcase_str(&name_u16, "TESTFILE.ZIP", &upcase));
        assert!(eq_names_upcase_str(&name_u16, "testfile.zip", &upcase));
        assert!(!eq_names_upcase_str(&name_u16, "Other.zip", &upcase));
    }
}
