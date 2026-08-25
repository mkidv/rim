// SPDX-License-Identifier: MIT
//! Shared Upcase Table handling for exFAT and NTFS.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{boxed::Box, vec, vec::Vec};

use crate::utils::checksum_utils::accumulate_checksum;

#[derive(Debug, Clone, PartialEq)]
pub enum UpcaseFlavor {
    /// Windows dump (NTFS/exFAT standard)
    Windows,
    /// Linux-ntfs/ntfs-3g Legacy generation
    Legacy,
    /// Custom table from raw bytes
    Custom,
}

/// Helper to accumulate checksum of a table
pub fn checksum_table_bytes(bytes: &[u8]) -> u32 {
    let mut checksum: u32 = 0;
    accumulate_checksum(&mut checksum, bytes);
    checksum
}

pub struct UpcaseHandle {
    table: Box<[u16]>,
    bytes: Box<[u8]>,
    len: usize,
    checksum: u32,
}

impl UpcaseHandle {
    pub const LEN_U16: usize = 65536;
    pub const LEN_BYTES: usize = 131072; // 128KB

    #[inline]
    pub fn upper(&self, cu: u16) -> u16 {
        let idx = cu as usize;
        if idx < self.table.len() {
            self.table[idx]
        } else {
            cu
        }
    }

    #[inline]
    pub fn checksum(&self) -> u32 {
        self.checksum
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[inline]
    pub fn as_u16(&self) -> &[u16] {
        &self.table
    }

    /// Read Upcase table from IO.
    ///
    /// generic over offset/size logic (handled by caller passing exact-sized reader or slice)
    pub fn from_bytes(blob: &[u8]) -> Result<Self, &'static str> {
        if !blob.len().is_multiple_of(2) {
            return Err("upcase_size_not_even");
        }

        let len = blob.len();
        let checksum = checksum_table_bytes(blob);

        let mut table = vec![0u16; len / 2].into_boxed_slice();
        for (i, ch) in blob.chunks_exact(2).enumerate() {
            table[i] = u16::from_le_bytes([ch[0], ch[1]]);
        }

        let mut bytes = vec![0u8; len].into_boxed_slice();
        bytes.copy_from_slice(blob);

        Ok(Self {
            table,
            bytes,
            len,
            checksum,
        })
    }

    pub fn from_u16_slice(src_table: &[u16]) -> Self {
        let len = src_table.len();
        let mut table = vec![0u16; len].into_boxed_slice();
        table.copy_from_slice(src_table);

        let bytes = serialize_u16_to_le_bytes(&table);
        let checksum = checksum_table_bytes(&bytes);
        let len = bytes.len();

        Self {
            table,
            bytes,
            len,
            checksum,
        }
    }

    /// Port of linux-ntfs `generate_default_upcase()` (legacy mapping set).
    pub fn generate_legacy() -> Self {
        // Start, End(exclusive), Add
        const UC_RUN_TABLE: &[(u16, u16, i16)] = &[
            (0x0061, 0x007B, -32),
            (0x0451, 0x045D, -80),
            (0x1F70, 0x1F72, 74),
            (0x00E0, 0x00F7, -32),
            (0x045E, 0x0460, -80),
            (0x1F72, 0x1F76, 86),
            (0x00F8, 0x00FF, -32),
            (0x0561, 0x0587, -48),
            (0x1F76, 0x1F78, 100),
            (0x0256, 0x0258, -205),
            (0x1F00, 0x1F08, 8),
            (0x1F78, 0x1F7A, 128),
            (0x028A, 0x028C, -217),
            (0x1F10, 0x1F16, 8),
            (0x1F7A, 0x1F7C, 112),
            (0x03AC, 0x03AD, -38),
            (0x1F20, 0x1F28, 8),
            (0x1F7C, 0x1F7E, 126),
            (0x03AD, 0x03B0, -37),
            (0x1F30, 0x1F38, 8),
            (0x1FB0, 0x1FB2, 8),
            (0x03B1, 0x03C2, -32),
            (0x1F40, 0x1F46, 8),
            (0x1FD0, 0x1FD2, 8),
            (0x03C2, 0x03C3, -31),
            (0x1F51, 0x1F52, 8),
            (0x1FE0, 0x1FE2, 8),
            (0x03C3, 0x03CC, -32),
            (0x1F53, 0x1F54, 8),
            (0x1FE5, 0x1FE6, 7),
            (0x03CC, 0x03CD, -64),
            (0x1F55, 0x1F56, 8),
            (0x2170, 0x2180, -16),
            (0x03CD, 0x03CF, -63),
            (0x1F57, 0x1F58, 8),
            (0x24D0, 0x24EA, -26),
            (0x0430, 0x0450, -32),
            (0x1F60, 0x1F68, 8),
            (0xFF41, 0xFF5B, -32),
        ];

        // Start, End(exclusive) - apply table[i+1] -= 1 for i in [start, end) step 2
        const UC_DUP_TABLE: &[(u16, u16)] = &[
            (0x0100, 0x012F),
            (0x01A0, 0x01A6),
            (0x03E2, 0x03EF),
            (0x04CB, 0x04CC),
            (0x0132, 0x0137),
            (0x01B3, 0x01B7),
            (0x0460, 0x0481),
            (0x04D0, 0x04EB),
            (0x0139, 0x0149),
            (0x01CD, 0x01DD),
            (0x0490, 0x04BF),
            (0x04EE, 0x04F5),
            (0x014A, 0x0178),
            (0x01DE, 0x01EF),
            (0x04BF, 0x04BF),
            (0x04F8, 0x04F9),
            (0x0179, 0x017E),
            (0x01F4, 0x01F5),
            (0x04C1, 0x04C4),
            (0x1E00, 0x1E95),
            (0x018B, 0x018B),
            (0x01FA, 0x0218),
            (0x04C7, 0x04C8),
            (0x1EA0, 0x1EF9),
        ];

        // Offset, Value
        const UC_WORD_TABLE: &[(u16, u16)] = &[
            (0x00FF, 0x0178),
            (0x01AD, 0x01AC),
            (0x01F3, 0x01F1),
            (0x0269, 0x0196),
            (0x0183, 0x0182),
            (0x01B0, 0x01AF),
            (0x0253, 0x0181),
            (0x026F, 0x019C),
            (0x0185, 0x0184),
            (0x01B9, 0x01B8),
            (0x0254, 0x0186),
            (0x0272, 0x019D),
            (0x0188, 0x0187),
            (0x01BD, 0x01BC),
            (0x0259, 0x018F),
            (0x0275, 0x019F),
            (0x018C, 0x018B),
            (0x01C6, 0x01C4),
            (0x025B, 0x0190),
            (0x0283, 0x01A9),
            (0x0192, 0x0191),
            (0x01C9, 0x01C7),
            (0x0260, 0x0193),
            (0x0288, 0x01AE),
            (0x0199, 0x0198),
            (0x01CC, 0x01CA),
            (0x0263, 0x0194),
            (0x0292, 0x01B7),
            (0x01A8, 0x01A7),
            (0x01DD, 0x018E),
            (0x0268, 0x0197),
        ];

        #[inline]
        fn add_i16(v: u16, delta: i16) -> u16 {
            // Wrap like 16-bit arithmetic
            (v as i32 + delta as i32) as u16
        }

        // 1) identity map
        let mut table: Vec<u16> = Vec::with_capacity(Self::LEN_U16);
        for i in 0u32..Self::LEN_U16 as u32 {
            table.push(i as u16);
        }

        // 2) run adjustments
        for &(start, end, delta) in UC_RUN_TABLE {
            let mut i = start as u32;
            let end_u = end as u32;
            while i < end_u {
                let idx = i as usize;
                table[idx] = add_i16(table[idx], delta);
                i += 1;
            }
        }

        // 3) dup adjustments
        for &(start, end) in UC_DUP_TABLE {
            let mut i = start as u32;
            let end_u = end as u32;
            while i < end_u {
                let idx = (i as usize) + 1;
                if idx < table.len() {
                    table[idx] = add_i16(table[idx], -1);
                }
                i += 2;
            }
        }

        // 4) word overrides
        for &(off, val) in UC_WORD_TABLE {
            table[off as usize] = val;
        }

        let table = table.into_boxed_slice();
        let bytes = serialize_u16_to_le_bytes(&table);
        let checksum = checksum_table_bytes(&bytes);

        Self {
            table,
            bytes,
            len: Self::LEN_BYTES,
            checksum,
        }
    }
}

#[inline]
fn serialize_u16_to_le_bytes(table: &[u16]) -> Box<[u8]> {
    let mut bytes = vec![0u8; table.len() * 2];
    for (i, &v) in table.iter().enumerate() {
        let [lo, hi] = v.to_le_bytes();
        bytes[i * 2] = lo;
        bytes[i * 2 + 1] = hi;
    }
    bytes.into_boxed_slice()
}
