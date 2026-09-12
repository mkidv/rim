// SPDX-License-Identifier: MIT
//! EXT4 Block Group Descriptor structure

use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// EXT4 Block Group Descriptor (64 bytes for 64-bit feature)
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
#[derive(Default)]
pub struct ExtBlockGroupDesc {
    pub bg_block_bitmap_lo: U32,
    pub bg_inode_bitmap_lo: U32,
    pub bg_inode_table_lo: U32,
    pub bg_free_blocks_count_lo: U16,
    pub bg_free_inodes_count_lo: U16,
    pub bg_used_dirs_count_lo: U16,
    pub bg_flags: U16,
    pub bg_exclude_bitmap_lo: U32,
    pub bg_block_bitmap_csum_lo: U16,
    pub bg_inode_bitmap_csum_lo: U16,
    pub bg_itable_unused_lo: U16,
    pub bg_checksum: U16,
    pub bg_block_bitmap_hi: U32,
    pub bg_inode_bitmap_hi: U32,
    pub bg_inode_table_hi: U32,
    pub bg_free_blocks_count_hi: U16,
    pub bg_free_inodes_count_hi: U16,
    pub bg_used_dirs_count_hi: U16,
    pub bg_itable_unused_hi: U16,
    pub bg_exclude_bitmap_hi: U32,
    pub bg_block_bitmap_csum_hi: U16,
    pub bg_inode_bitmap_csum_hi: U16,
    pub bg_reserved: U32,
}

impl ExtBlockGroupDesc {
    /// Create a new block group descriptor
    pub fn new(
        block_bitmap: u64,
        inode_bitmap: u64,
        inode_table: u64,
        free_blocks: u16,
        free_inodes: u16,
        used_dirs: u16,
    ) -> Self {
        Self {
            bg_block_bitmap_lo: (block_bitmap as u32).into(),
            bg_inode_bitmap_lo: (inode_bitmap as u32).into(),
            bg_inode_table_lo: (inode_table as u32).into(),
            bg_free_blocks_count_lo: free_blocks.into(),
            bg_free_inodes_count_lo: free_inodes.into(),
            bg_used_dirs_count_lo: used_dirs.into(),
            bg_block_bitmap_hi: ((block_bitmap >> 32) as u32).into(),
            bg_inode_bitmap_hi: ((inode_bitmap >> 32) as u32).into(),
            bg_inode_table_hi: ((inode_table >> 32) as u32).into(),
            ..Default::default()
        }
    }

    /// Get free blocks count (combined lo + hi)
    pub fn free_blocks(&self) -> u32 {
        self.bg_free_blocks_count_lo.get() as u32
            | ((self.bg_free_blocks_count_hi.get() as u32) << 16)
    }

    /// Get free inodes count (combined lo + hi)
    pub fn free_inodes(&self) -> u32 {
        self.bg_free_inodes_count_lo.get() as u32
            | ((self.bg_free_inodes_count_hi.get() as u32) << 16)
    }

    /// Get free blocks count respecting 64-bit feature flag
    pub fn free_blocks_ext(&self, has_64bit: bool) -> u32 {
        let lo = self.bg_free_blocks_count_lo.get() as u32;
        let hi = if has_64bit {
            self.bg_free_blocks_count_hi.get() as u32
        } else {
            0
        };
        lo | (hi << 16)
    }

    /// Get free inodes count respecting 64-bit feature flag
    pub fn free_inodes_ext(&self, has_64bit: bool) -> u32 {
        let lo = self.bg_free_inodes_count_lo.get() as u32;
        let hi = if has_64bit {
            self.bg_free_inodes_count_hi.get() as u32
        } else {
            0
        };
        lo | (hi << 16)
    }

    /// Get block bitmap block (combined lo + hi)
    pub fn block_bitmap(&self, has_64bit: bool) -> u64 {
        let lo = self.bg_block_bitmap_lo.get() as u64;
        let hi = if has_64bit {
            self.bg_block_bitmap_hi.get() as u64
        } else {
            0
        };
        lo | (hi << 32)
    }

    /// Get inode bitmap block (combined lo + hi)
    pub fn inode_bitmap(&self, has_64bit: bool) -> u64 {
        let lo = self.bg_inode_bitmap_lo.get() as u64;
        let hi = if has_64bit {
            self.bg_inode_bitmap_hi.get() as u64
        } else {
            0
        };
        lo | (hi << 32)
    }

    /// Get inode table block (combined lo + hi)
    pub fn inode_table(&self, has_64bit: bool) -> u64 {
        let lo = self.bg_inode_table_lo.get() as u64;
        let hi = if has_64bit {
            self.bg_inode_table_hi.get() as u64
        } else {
            0
        };
        lo | (hi << 32)
    }

    /// Encode to raw bytes
    pub fn to_bytes(&self) -> [u8; 64] {
        // Safe: ExtBlockGroupDesc is exactly 64 bytes by layout and static assert
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("ExtBlockGroupDesc size mismatch")
    }
}

// Ensure the struct is exactly 64 bytes
const _: () = assert!(core::mem::size_of::<ExtBlockGroupDesc>() == 64);

const _: () = {
    assert!(core::mem::align_of::<ExtBlockGroupDesc>() == 1);
    assert!(core::mem::offset_of!(ExtBlockGroupDesc, bg_checksum) == 30);
    assert!(core::mem::offset_of!(ExtBlockGroupDesc, bg_block_bitmap_hi) == 32);
};

/// Partial BGDT update (6 bytes at offset 0x0C in each descriptor)
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, Default)]
#[repr(C)]
pub struct ExtBgdtUpdate {
    pub bg_free_blocks_count_lo: U16,
    pub bg_free_inodes_count_lo: U16,
    pub bg_used_dirs_count_lo: U16,
}

impl ExtBgdtUpdate {
    /// Create a new BGDT update with the given counts
    pub fn new(free_blocks: u16, free_inodes: u16, used_dirs: u16) -> Self {
        Self {
            bg_free_blocks_count_lo: free_blocks.into(),
            bg_free_inodes_count_lo: free_inodes.into(),
            bg_used_dirs_count_lo: used_dirs.into(),
        }
    }
}

// Ensure the struct is exactly 6 bytes
const _: () = assert!(core::mem::size_of::<ExtBgdtUpdate>() == 6);

#[cfg(test)]
mod endian_tests {
    use super::*;
    #[test]
    fn descriptor_bytes_preserve_high_addresses() {
        let desc = ExtBlockGroupDesc::new(0x1122334455667788, 0, 0, 0x1234, 0, 0);
        assert_eq!(&desc.as_bytes()[..4], &[0x88, 0x77, 0x66, 0x55]);
        assert_eq!(&desc.as_bytes()[12..14], &[0x34, 0x12]);
        assert_eq!(&desc.as_bytes()[32..36], &[0x44, 0x33, 0x22, 0x11]);
    }
}
