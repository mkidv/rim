// SPDX-License-Identifier: MIT
//! EXT4 Block Group Descriptor partial update structure
//!
//! Used by the injector to update only the free blocks/inodes/dirs counts
//! in each BGDT entry without rewriting the entire descriptor.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Partial BGDT update (6 bytes at offset 0x0C in each descriptor)
///
/// This struct matches the layout of fields at offset 0x0C-0x12 in
/// `Ext4BlockGroupDesc`:
/// - `bg_free_blocks_count_lo` (u16)
/// - `bg_free_inodes_count_lo` (u16)
/// - `bg_used_dirs_count_lo` (u16)
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct Ext4BgdtUpdate {
    pub bg_free_blocks_count_lo: u16,
    pub bg_free_inodes_count_lo: u16,
    pub bg_used_dirs_count_lo: u16,
}

impl Ext4BgdtUpdate {
    /// Create a new BGDT update with the given counts
    pub fn new(free_blocks: u16, free_inodes: u16, used_dirs: u16) -> Self {
        Self {
            bg_free_blocks_count_lo: free_blocks,
            bg_free_inodes_count_lo: free_inodes,
            bg_used_dirs_count_lo: used_dirs,
        }
    }
}

// Ensure the struct is exactly 6 bytes
const _: () = assert!(core::mem::size_of::<Ext4BgdtUpdate>() == 6);
