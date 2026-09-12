// SPDX-License-Identifier: MIT

//! ext4 extent header, extent index, and extent leaf on-disk structures.

use crate::constant::EXT_EXTENT_HEADER_MAGIC;
use rimio::prelude::*;
use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ExtExtentHeader {
    pub eh_magic: U16,
    pub eh_entries: U16,
    pub eh_max: U16,
    pub eh_depth: U16,
    pub eh_generation: U32,
}

impl Default for ExtExtentHeader {
    fn default() -> Self {
        Self {
            eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
            eh_entries: 0.into(),
            eh_max: 4.into(),
            eh_depth: 0.into(),
            eh_generation: 0.into(),
        }
    }
}

/// Leaf node entry
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ExtExtent {
    pub ee_block: U32,
    pub ee_len: U16,
    pub ee_start_hi: U16,
    pub ee_start_lo: U32,
}

impl ExtExtent {
    pub fn new(logical: u32, physical: u32, len: u16) -> Self {
        Self::new_48(logical, physical as u64, len)
    }

    pub fn new_48(logical: u32, physical: u64, len: u16) -> Self {
        Self {
            ee_block: logical.into(),
            ee_len: len.into(),
            ee_start_hi: (((physical >> 32) & 0xFFFF) as u16).into(),
            ee_start_lo: ((physical & 0xFFFF_FFFF) as u32).into(),
        }
    }

    #[inline(always)]
    pub fn physical_start(&self) -> u64 {
        ((self.ee_start_hi.get() as u64) << 32) | (self.ee_start_lo.get() as u64)
    }

    /// Check if extent is uninitialized / preallocated (ee_len > 32768)
    #[inline(always)]
    pub fn is_uninit(&self) -> bool {
        self.ee_len.get() > 32768
    }

    /// Get extent block length.
    /// Per ext4 spec: if ee_len <= 32768, length is ee_len.
    /// If ee_len > 32768, extent is uninitialized and length is ee_len - 32768.
    #[inline(always)]
    pub fn len(&self) -> u16 {
        if self.ee_len.get() > 32768 {
            self.ee_len.get() - 32768
        } else {
            self.ee_len.get()
        }
    }

    /// Check if extent length is 0
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl From<MappedRun> for ExtExtent {
    fn from(run: MappedRun) -> Self {
        Self::new_48(
            run.logical_offset as u32,
            run.physical.start,
            run.physical.length as u16,
        )
    }
}

/// Index node entry (internal node)
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ExtExtentIndex {
    pub ei_block: U32,
    pub ei_leaf_lo: U32,
    pub ei_leaf_hi: U16,
    pub ei_unused: U16,
}

impl ExtExtentIndex {
    pub fn new(logical: u32, physical_next_level: u32) -> Self {
        Self::new_48(logical, physical_next_level as u64)
    }

    pub fn new_48(logical: u32, physical_next_level: u64) -> Self {
        Self {
            ei_block: logical.into(),
            ei_leaf_lo: ((physical_next_level & 0xFFFF_FFFF) as u32).into(),
            ei_leaf_hi: (((physical_next_level >> 32) & 0xFFFF) as u16).into(),
            ei_unused: 0.into(),
        }
    }

    #[inline(always)]
    pub fn leaf_physical_block(&self) -> u64 {
        ((self.ei_leaf_hi.get() as u64) << 32) | (self.ei_leaf_lo.get() as u64)
    }
}

const _: () = {
    assert!(core::mem::size_of::<ExtExtentHeader>() == 12);
    assert!(core::mem::size_of::<ExtExtent>() == 12);
    assert!(core::mem::size_of::<ExtExtentIndex>() == 12);
    assert!(core::mem::align_of::<ExtExtent>() == 1);
    assert!(core::mem::offset_of!(ExtExtent, ee_len) == 4);
    assert!(core::mem::offset_of!(ExtExtent, ee_start_hi) == 6);
    assert!(core::mem::offset_of!(ExtExtentIndex, ei_leaf_hi) == 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extent_bytes_preserve_48_bit_addresses() {
        let extent = ExtExtent::new_48(0x12345678, 0xabcd11223344, 0x8002);
        assert_eq!(
            extent.as_bytes(),
            &[
                0x78, 0x56, 0x34, 0x12, 2, 0x80, 0xcd, 0xab, 0x44, 0x33, 0x22, 0x11
            ]
        );
        let index = ExtExtentIndex::new_48(0x12345678, 0xabcd11223344);
        assert_eq!(
            index.as_bytes(),
            &[
                0x78, 0x56, 0x34, 0x12, 0x44, 0x33, 0x22, 0x11, 0xcd, 0xab, 0, 0
            ]
        );
        let mut bytes = [0; 13];
        bytes[1..].copy_from_slice(extent.as_bytes());
        let view = ExtExtent::ref_from_bytes(&bytes[1..]).unwrap();
        assert_eq!(view.physical_start(), 0xabcd11223344);
        assert!(view.is_uninit());
        assert_eq!(view.len(), 2);
        assert!(ExtExtent::ref_from_bytes(&bytes[1..12]).is_err());
        assert_eq!(
            &ExtExtentHeader::default().as_bytes()[..8],
            &[0x0a, 0xf3, 0, 0, 4, 0, 0, 0]
        );
    }

    #[test]
    fn test_extent_init_vs_uninit_boundary() {
        // Initialized extent with exactly 32768 blocks (maximum initialized extent length)
        let ext_32768 = ExtExtent::new(0, 100, 32768);
        assert!(!ext_32768.is_uninit());
        assert_eq!(ext_32768.len(), 32768);

        // Standard small initialized extent
        let ext_small = ExtExtent::new(0, 100, 10);
        assert!(!ext_small.is_uninit());
        assert_eq!(ext_small.len(), 10);

        // Uninitialized extent: ee_len = 32768 + 2 = 32770
        let ext_uninit = ExtExtent::new(0, 100, 32770);
        assert!(ext_uninit.is_uninit());
        assert_eq!(ext_uninit.len(), 2);
    }
}
