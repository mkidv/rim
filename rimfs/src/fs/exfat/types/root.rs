// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::fs::exfat::constant::*;

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatBitmapEntry {
    pub entry_type: u8,
    pub bitmap_flags: u8,
    pub reserved: [u8; 18],
    pub first_cluster: u32,
    pub data_length: u64,
}

impl ExFatBitmapEntry {
    pub fn new(first_cluster: u32, data_length: u64) -> Self {
        Self {
            entry_type: EXFAT_ENTRY_BITMAP,
            bitmap_flags: 0,
            reserved: [0u8; 18],
            first_cluster,
            data_length,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatUpcaseEntry {
    pub entry_type: u8,
    pub reserved1: [u8; 3],
    pub table_checksum: u32,
    pub reserved2: [u8; 12],
    pub first_cluster: u32,
    pub data_length: u64,
}

impl ExFatUpcaseEntry {
    pub fn new(first_cluster: u32, table_len: u64, table_checksum: u32) -> Self {
        Self {
            entry_type: EXFAT_ENTRY_UPCASE,
            reserved1: [0u8; 3],
            table_checksum,
            reserved2: [0u8; 12],
            first_cluster,
            data_length: table_len,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatVolumeLabelEntry {
    pub entry_type: u8,
    pub character_count: u8,
    pub volume_label: [u16; 11],
    pub reserved: u64,
}

impl ExFatVolumeLabelEntry {
    pub fn new(volume_label: [u16; 11]) -> Self {
        let character_count = volume_label.iter().take_while(|&&c| c != 0).count() as u8;

        Self {
            entry_type: EXFAT_ENTRY_LABEL,
            character_count,
            volume_label,
            reserved: 0,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, Immutable)]
#[repr(C, packed)]
pub struct ExFatGuidEntry {
    pub entry_type: u8,
    pub secondary_count: u8,
    pub set_checksum: u16,
    pub general_primary_flags: u16,
    pub guid: [u8; 16],
    pub reserved: [u8; 10],
}

impl ExFatGuidEntry {
    pub fn new(guid: [u8; 16]) -> Self {
        let mut entry = Self {
            entry_type: EXFAT_ENTRY_GUID,
            secondary_count: 0,
            set_checksum: 0,
            general_primary_flags: 0,
            guid,
            reserved: [0u8; 10],
        };
        entry.compute_set_checksum();
        entry
    }

    pub fn new_placeholder() -> Self {
        let mut entry = Self {
            entry_type: EXFAT_ENTRY_GUID & !EXFAT_ENTRY_INVAL,
            secondary_count: 0,
            set_checksum: 0,
            general_primary_flags: 0,
            guid: [1u8; 16],
            reserved: [0u8; 10],
        };
        entry.compute_set_checksum();
        entry
    }

    fn compute_set_checksum(&mut self) {
        let mut sum = 0u16;
        let arr = self.as_bytes();
        for (i, &b) in arr.iter().enumerate() {
            if i == 2 || i == 3 {
                continue;
            }
            sum = sum.wrapping_add(b as u16);
        }

        self.set_checksum = sum;
    }

    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}
