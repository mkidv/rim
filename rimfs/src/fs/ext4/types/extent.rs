// SPDX-License-Identifier: MIT
use crate::fs::ext4::constant::EXT4_EXTENT_HEADER_MAGIC;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct Ext4ExtentHeader {
    pub eh_magic: u16,      // Magic value EXT4_EXTENT_HEADER_MAGIC
    pub eh_entries: u16,    // Number of valid entries
    pub eh_max: u16,        // Capacity of storage in this header
    pub eh_depth: u16,      // 0 = leaf node, > 0 = index node
    pub eh_generation: u32, // Generation of the tree
}

impl Default for Ext4ExtentHeader {
    fn default() -> Self {
        Self {
            eh_magic: EXT4_EXTENT_HEADER_MAGIC,
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
pub struct Ext4Extent {
    pub ee_block: u32,    // First logical block extent covers
    pub ee_len: u16,      // Number of blocks covered by extent
    pub ee_start_hi: u16, // High 16 bits of physical block
    pub ee_start_lo: u32, // Low 32 bits of physical block
}

impl Ext4Extent {
    pub fn new(logical: u32, physical: u32, len: u16) -> Self {
        Self {
            ee_block: logical,
            ee_len: len,
            ee_start_hi: ((physical as u64) >> 32) as u16,
            ee_start_lo: physical,
        }
    }
}

/// Index node entry (internal node)
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct Ext4ExtentIndex {
    pub ei_block: u32,   // Index covers logical blocks from 'block'
    pub ei_leaf_lo: u32, // Low 32 bits of physical block of the next level
    pub ei_leaf_hi: u16, // High 16 bits of physical block of the next level
    pub ei_unused: u16,
}

impl Ext4ExtentIndex {
    pub fn new(logical: u32, physical_next_level: u32) -> Self {
        Self {
            ei_block: logical,
            ei_leaf_lo: physical_next_level,
            ei_leaf_hi: ((physical_next_level as u64) >> 32) as u16,
            ei_unused: 0,
        }
    }
}
