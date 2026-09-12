// SPDX-License-Identifier: MIT
//! NTFS Data Run encoding and decoding.
//!
//! Non-resident attribute streams are allocated via Run Lists encoded as a series of
//! variable-length (length, LCN offset) tuples.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::flags::DataRunHeader;

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
    let lcn_size = if lcn == 0 {
        lcn_buf[0] = 0;
        1
    } else {
        encode_variable_int_to_buf(lcn, true, &mut lcn_buf)
    };
    result[cursor..cursor + lcn_size].copy_from_slice(&lcn_buf[..lcn_size]);
    cursor += lcn_size;

    // Header byte: high nibble = LCN size, low nibble = length size
    let header = DataRunHeader::new(len_size as u8, lcn_size as u8);
    result[0] = header.raw();

    (result, cursor)
}

/// Encode an integer in variable-length format into a fixed buffer
fn encode_variable_int_to_buf(value: i64, signed: bool, buf: &mut [u8; 8]) -> usize {
    if value == 0 {
        buf[0] = 0;
        return 0;
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
            *byte = (v & 0xFF) as u8;
            v >>= 8;
            len = i + 1;
            if v == 0 && (*byte & 0x80) == 0 {
                break;
            }
        }
    }

    len
}

/// Terminate a data run sequence
pub fn encode_data_run_end() -> u8 {
    0x00
}

/// Encode a complete RunList into a sequence of raw NTFS data runs (including 0x00 terminator).
pub fn encode_runs_to_dataruns(runs: &rimio::run::RunList) -> Vec<u8> {
    let mut out = Vec::new();
    let mut last_lcn = 0i64;

    for run in runs.iter() {
        let delta = run.start as i64 - last_lcn;
        let (enc, len) = encode_data_run(delta, run.length);
        out.extend_from_slice(&enc[..len]);
        last_lcn = run.start as i64;
    }
    out.push(encode_data_run_end());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_run_encoding() {
        let (run, len) = encode_data_run(100, 10);
        assert!(len > 0);
        assert_eq!(run[0] & 0x0F, 1);

        let (run128, len128) = encode_data_run(4101, 128);
        assert_eq!(
            run128[0] & 0x0F,
            2,
            "Length 128 must use 2 bytes (0x80, 0x00)"
        );
        assert_eq!(run128[1], 0x80);
        let mut full_runs = run128[..len128].to_vec();
        full_runs.push(0);
        let runs: Vec<_> = crate::view::runlist::NtfsRunList::new(&full_runs)
            .iter()
            .collect();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len, 128);
        assert_eq!(runs[0].lcn, Some(4101));
    }
}
