// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use crate::core::traits::FileAttributes;
use crate::fs::ext4::constant::*;
use crate::fs::ext4::types::{Ext4DirEntry, Ext4Extent, Ext4Inode};

/// Helper to manage the `lost+found` directory
/// This is typically created at inode 11 within the root directory.
pub struct Ext4LostFound;

impl Ext4LostFound {
    /// Inode number for lost+found (11)
    pub const INODE: u32 = EXT4_FIRST_INODE;
    pub const NAME: &'static str = "lost+found";

    /// Generate the directory block content for `lost+found` (contains `.` and `..`)
    ///
    /// `block_size`: Filesystem block size
    /// `block_id`: The block number allocated for this directory
    pub fn create_dir_block(block_size: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(block_size);

        // "." entry pointing to itself (inode 11)
        Ext4DirEntry::dot(Self::INODE).to_raw_buffer(&mut buf);

        // ".." entry pointing to root (inode 2)
        Ext4DirEntry::dotdot(EXT4_ROOT_INODE).to_raw_buffer(&mut buf);

        // Pad the rest of the block
        // Ext4 requires the last entry to span the rest of the block,
        // so we adjust the last entry's rec_len implicitly or explicitly.
        // `Ext4DirEntry::to_raw_buffer` handles partial padding, but we need
        // to make sure we fill the block.

        if buf.len() < block_size {
            let padding = block_size - buf.len();
            buf.extend(core::iter::repeat_n(0, padding));
        }

        // Fix up the last entry's rec_len to cover the whole block
        // e2fsck requirement: The directory block must be fully covered by entries.
        // The last entry (typically "..") is extended.
        Self::pad_last_entry(&mut buf, block_size);

        buf
    }

    /// Helper to adjust the last entry's rec_len to cover the rest of the block.
    fn pad_last_entry(buf: &mut [u8], block_size: usize) {
        if buf.is_empty() {
            return;
        }

        let mut pos = 0;
        let mut last_entry_pos = 0;

        // Walk entries to find the last one
        while pos + 8 <= buf.len() {
            let rec_len = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
            if rec_len == 0 {
                break;
            }
            // Sanity check preventing infinite loop if corrupt
            if pos + rec_len > buf.len() {
                break;
            }
            last_entry_pos = pos;
            pos += rec_len;
        }

        let remaining = block_size - last_entry_pos;
        if remaining > 0 && remaining <= 65535 {
            let new_len = remaining as u16;
            buf[last_entry_pos + 4] = (new_len & 0xFF) as u8;
            buf[last_entry_pos + 5] = ((new_len >> 8) & 0xFF) as u8;
        }
    }

    /// Generate the Inode for `lost+found`
    pub fn create_inode(block_size: u32, block_id: u32) -> Ext4Inode {
        let extent = Ext4Extent::new(0, block_id, 1);
        let attr = FileAttributes {
            mode: Some(0o700), // drwx------
            dir: true,
            ..Default::default()
        };

        Ext4Inode::from_attr(
            &attr,
            block_size as u64,
            2, // Links: . and .. (from subdirs, but initially empty so just 2)
            block_size.div_ceil(512),
            &[extent],
        )
    }

    /// Create the directory entry for `lost+found` to be placed in the parent (Root)
    pub fn entry() -> Ext4DirEntry {
        Ext4DirEntry::dir(Self::INODE, Self::NAME)
    }
}
