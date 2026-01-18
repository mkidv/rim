// SPDX-License-Identifier: MIT
//! EXT4 Block Group Descriptor structure

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// EXT4 Block Group Descriptor (64 bytes for 64-bit feature)
///
/// This represents an on-disk block group descriptor entry.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
#[derive(Default)]
pub struct Ext4BlockGroupDesc {
    /// Block bitmap block (lower 32 bits)
    pub bg_block_bitmap_lo: u32,
    /// Inode bitmap block (lower 32 bits)
    pub bg_inode_bitmap_lo: u32,
    /// Inode table block (lower 32 bits)
    pub bg_inode_table_lo: u32,
    /// Free blocks count (lower 16 bits)
    pub bg_free_blocks_count_lo: u16,
    /// Free inodes count (lower 16 bits)
    pub bg_free_inodes_count_lo: u16,
    /// Used directories count (lower 16 bits)
    pub bg_used_dirs_count_lo: u16,
    /// Block group flags
    pub bg_flags: u16,
    /// Exclude bitmap block (lower 32 bits)
    pub bg_exclude_bitmap_lo: u32,
    /// Block bitmap checksum (lower 16 bits)
    pub bg_block_bitmap_csum_lo: u16,
    /// Inode bitmap checksum (lower 16 bits)
    pub bg_inode_bitmap_csum_lo: u16,
    /// Unused inode count (lower 16 bits)
    pub bg_itable_unused_lo: u16,
    /// Group descriptor checksum
    pub bg_checksum: u16,
    // 64-bit extensions (when desc_size >= 64)
    /// Block bitmap block (upper 32 bits)
    pub bg_block_bitmap_hi: u32,
    /// Inode bitmap block (upper 32 bits)
    pub bg_inode_bitmap_hi: u32,
    /// Inode table block (upper 32 bits)
    pub bg_inode_table_hi: u32,
    /// Free blocks count (upper 16 bits)
    pub bg_free_blocks_count_hi: u16,
    /// Free inodes count (upper 16 bits)
    pub bg_free_inodes_count_hi: u16,
    /// Used directories count (upper 16 bits)
    pub bg_used_dirs_count_hi: u16,
    /// Unused inode count (upper 16 bits)
    pub bg_itable_unused_hi: u16,
    /// Exclude bitmap block (upper 32 bits)
    pub bg_exclude_bitmap_hi: u32,
    /// Block bitmap checksum (upper 16 bits)
    pub bg_block_bitmap_csum_hi: u16,
    /// Inode bitmap checksum (upper 16 bits)
    pub bg_inode_bitmap_csum_hi: u16,
    /// Reserved
    pub bg_reserved: u32,
}

impl Ext4BlockGroupDesc {
    /// Create a new block group descriptor
    pub fn new(
        block_bitmap: u32,
        inode_bitmap: u32,
        inode_table: u32,
        free_blocks: u16,
        free_inodes: u16,
        used_dirs: u16,
    ) -> Self {
        Self {
            bg_block_bitmap_lo: block_bitmap,
            bg_inode_bitmap_lo: inode_bitmap,
            bg_inode_table_lo: inode_table,
            bg_free_blocks_count_lo: free_blocks,
            bg_free_inodes_count_lo: free_inodes,
            bg_used_dirs_count_lo: used_dirs,
            ..Default::default()
        }
    }

    /// Get free blocks count (combined lo + hi)
    pub fn free_blocks(&self) -> u32 {
        self.bg_free_blocks_count_lo as u32 | ((self.bg_free_blocks_count_hi as u32) << 16)
    }

    /// Get free inodes count (combined lo + hi)
    pub fn free_inodes(&self) -> u32 {
        self.bg_free_inodes_count_lo as u32 | ((self.bg_free_inodes_count_hi as u32) << 16)
    }

    /// Encode to raw bytes
    pub fn to_bytes(&self) -> [u8; 64] {
        // Safe: Ext4BlockGroupDesc is exactly 64 bytes by layout and static assert
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("Ext4BlockGroupDesc size mismatch")
    }
}

// Ensure the struct is exactly 64 bytes
const _: () = assert!(core::mem::size_of::<Ext4BlockGroupDesc>() == 64);
