// SPDX-License-Identifier: MIT
use crate::constant::EXT_EXTENT_HEADER_MAGIC;
use rimio::prelude::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct ExtExtentHeader {
    pub eh_magic: u16,      // Magic value EXT_EXTENT_HEADER_MAGIC
    pub eh_entries: u16,    // Number of valid entries
    pub eh_max: u16,        // Capacity of storage in this header
    pub eh_depth: u16,      // 0 = leaf node, > 0 = index node
    pub eh_generation: u32, // Generation of the tree
}

impl Default for ExtExtentHeader {
    fn default() -> Self {
        Self {
            eh_magic: EXT_EXTENT_HEADER_MAGIC,
            eh_entries: 0,
            eh_max: 4, // Default for inode body (can be 3 or 4 depending on overhead)
            eh_depth: 0,
            eh_generation: 0,
        }
    }
}

/// Leaf node entry
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct ExtExtent {
    pub ee_block: u32,    // First logical block extent covers
    pub ee_len: u16,      // Number of blocks covered by extent
    pub ee_start_hi: u16, // High 16 bits of physical block
    pub ee_start_lo: u32, // Low 32 bits of physical block
}

impl ExtExtent {
    pub fn new(logical: u32, physical: u32, len: u16) -> Self {
        Self::new_48(logical, physical as u64, len)
    }

    pub fn new_48(logical: u32, physical: u64, len: u16) -> Self {
        Self {
            ee_block: logical,
            ee_len: len,
            ee_start_hi: ((physical >> 32) & 0xFFFF) as u16,
            ee_start_lo: (physical & 0xFFFF_FFFF) as u32,
        }
    }

    #[inline(always)]
    pub fn physical_start(&self) -> u64 {
        ((self.ee_start_hi as u64) << 32) | (self.ee_start_lo as u64)
    }

    /// Check if extent is uninitialized / preallocated (bit 15 of ee_len is set)
    #[inline(always)]
    pub fn is_uninit(&self) -> bool {
        (self.ee_len & 0x8000) != 0 || self.ee_len > 32768
    }

    /// Get extent block length (clearing uninitialized bit if present)
    pub fn len(&self) -> u16 {
        self.ee_len & 0x7FFF
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
#[repr(C, packed)]
pub struct ExtExtentIndex {
    pub ei_block: u32,   // Index covers logical blocks from 'block'
    pub ei_leaf_lo: u32, // Low 32 bits of physical block of the next level
    pub ei_leaf_hi: u16, // High 16 bits of physical block of the next level
    pub ei_unused: u16,
}

impl ExtExtentIndex {
    pub fn new(logical: u32, physical_next_level: u32) -> Self {
        Self::new_48(logical, physical_next_level as u64)
    }

    pub fn new_48(logical: u32, physical_next_level: u64) -> Self {
        Self {
            ei_block: logical,
            ei_leaf_lo: (physical_next_level & 0xFFFF_FFFF) as u32,
            ei_leaf_hi: ((physical_next_level >> 32) & 0xFFFF) as u16,
            ei_unused: 0,
        }
    }

    #[inline(always)]
    pub fn leaf_physical_block(&self) -> u64 {
        ((self.ei_leaf_hi as u64) << 32) | (self.ei_leaf_lo as u64)
    }
}
