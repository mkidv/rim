// SPDX-License-Identifier: MIT
//! MFT 64-bit reference and sequence number helpers.
//!
//! In NTFS, an MFT reference is a 64-bit value where the lower 48 bits are the record number
//! and the upper 16 bits are the sequence number.

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

/// Returns the canonical sequence number for an MFT record number.
pub fn mft_record_sequence_number(mft_num: u64) -> u16 {
    match mft_num {
        0 | 1 => 1,
        2..=15 => mft_num as u16,
        _ => 1,
    }
}

/// Build an MFT reference using the canonical sequence number.
pub fn system_file_mft_reference(mft_num: u64) -> u64 {
    build_mft_reference(mft_num, mft_record_sequence_number(mft_num))
}
