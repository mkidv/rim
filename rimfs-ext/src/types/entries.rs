// SPDX-License-Identifier: MIT
//! EXT Directory Entry structure

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    FileAttributes,
    {
        attr::ExtFileAttributesExt,
        constant::*,
        types::{ExtExtent, ExtInode},
    },
};

/// EXT Directory Entry header (fixed 8 bytes)
///
/// This is the fixed-size header portion of a directory entry.
/// The name field follows immediately after and is variable length.
#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExtDirEntryHeader {
    /// Inode number
    pub inode: u32,
    /// Record length (total size of this entry including padding)
    pub rec_len: u16,
    /// Name length (excluding null terminator)
    pub name_len: u8,
    /// File type (EXT_FT_* constants)
    pub file_type: u8,
}

/// EXT Directory Entry structure
///
/// This represents an on-disk directory entry for EXT filesystems.
/// The structure is variable-length with a minimum of 8 bytes header.
#[derive(Debug, Clone)]
pub struct ExtDirEntry {
    /// Inode number
    pub inode: u32,
    /// Record length (total size of this entry including padding)
    pub rec_len: u16,
    /// Name length (excluding null terminator)
    pub name_len: u8,
    /// File type (EXT_FT_* constants)
    pub file_type: u8,
    /// Entry name (variable length)
    pub name: Vec<u8>,
}

impl ExtDirEntry {
    /// Create a new directory entry
    pub fn new(inode: u32, name: &str, file_type: u8) -> Self {
        let name_bytes = name.as_bytes().to_vec();
        let name_len = name_bytes.len() as u8;
        // Record length = 8 (header) + name_len, rounded up to 4-byte boundary
        let rec_len = ((8 + name_len as usize).div_ceil(4) * 4) as u16;

        Self {
            inode,
            rec_len,
            name_len,
            file_type,
            name: name_bytes,
        }
    }

    /// Create a "." entry for a directory
    pub fn dot(current_inode: u32) -> Self {
        Self::new(current_inode, ".", EXT_FT_DIR)
    }

    /// Create a ".." entry for a directory
    pub fn dotdot(parent_inode: u32) -> Self {
        Self::new(parent_inode, "..", EXT_FT_DIR)
    }

    /// Create an entry for a subdirectory
    pub fn dir(inode: u32, name: &str) -> Self {
        Self::new(inode, name, EXT_FT_DIR)
    }

    /// Create an entry for a regular file
    pub fn file(inode: u32, name: &str) -> Self {
        Self::new(inode, name, EXT_FT_REG_FILE)
    }

    /// Set record length (for filling remaining space in directory block)
    pub fn set_rec_len(&mut self, len: u16) {
        self.rec_len = len;
    }

    /// Get the minimum record length for this entry
    pub fn min_rec_len(&self) -> u16 {
        ((8 + self.name_len as usize).div_ceil(4) * 4) as u16
    }

    /// Encode to bytes for writing to disk
    /// Uses ExtDirEntryHeader for the fixed header, then appends name
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        let header = ExtDirEntryHeader {
            inode: self.inode,
            rec_len: self.rec_len,
            name_len: self.name_len,
            file_type: self.file_type,
        };
        buf.extend_from_slice(header.as_bytes());

        // name (variable)
        buf.extend_from_slice(&self.name);

        // Padding to rec_len
        let current_len = 8 + self.name.len();
        if self.rec_len as usize > current_len {
            let padding = self.rec_len as usize - current_len;
            buf.extend(core::iter::repeat_n(0, padding));
        }
    }

    /// Parse from raw bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let inode = u32::from_le_bytes(data[0..4].try_into().ok()?);
        let rec_len = u16::from_le_bytes(data[4..6].try_into().ok()?);
        let name_len = data[6];
        let file_type = data[7];

        if data.len() < 8 + name_len as usize {
            return None;
        }

        let name = data[8..8 + name_len as usize].to_vec();

        Some(Self {
            inode,
            rec_len,
            name_len,
            file_type,
            name,
        })
    }

    /// Get name as string
    pub fn name_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.name).ok()
    }

    /// Check if this is a directory entry
    pub fn is_dir(&self) -> bool {
        self.file_type == EXT_FT_DIR
    }

    /// Check if this is a file entry
    pub fn is_file(&self) -> bool {
        self.file_type == EXT_FT_REG_FILE
    }

    /// Check if this is an empty/deleted entry
    pub fn is_empty(&self) -> bool {
        self.inode == 0
    }

    /// Create a directory entry from FileAttributes
    pub fn from_attr(inode: u32, name: &str, attr: &crate::core::traits::FileAttributes) -> Self {
        let file_type = attr.as_ext4_file_type();
        Self::new(inode, name, file_type)
    }
}

/// Helper to manage the `lost+found` directory
/// This is typically created at inode 11 within the root directory.
pub struct ExtLostFound;

impl ExtLostFound {
    /// Inode number for lost+found (11)
    pub const INODE: u32 = EXT_FIRST_INODE;
    pub const NAME: &'static str = "lost+found";

    /// Generate the directory block content for `lost+found` (contains `.` and `..`)
    ///
    /// `block_size`: Filesystem block size
    /// `block_id`: The block number allocated for this directory
    pub fn create_dir_block(block_size: usize) -> Vec<u8> {
        let mut buf = Vec::with_capacity(block_size);

        // "." entry pointing to itself (inode 11)
        ExtDirEntry::dot(Self::INODE).to_raw_buffer(&mut buf);

        // ".." entry pointing to root (inode 2)
        ExtDirEntry::dotdot(EXT_ROOT_INODE).to_raw_buffer(&mut buf);

        // Pad the rest of the block
        // Ext requires the last entry to span the rest of the block,
        // so we adjust the last entry's rec_len implicitly or explicitly.
        // `ExtDirEntry::to_raw_buffer` handles partial padding, but we need
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
    pub fn create_inode(block_size: u32, block_id: u32) -> ExtInode {
        let extent = ExtExtent::new(0, block_id, 1);
        let mut attr = FileAttributes::new_dir();
        attr.mode = Some(0o700); // drwx------

        ExtInode::from_attr(
            &attr,
            block_size as u64,
            2, // Links: . and .. (from subdirs, but initially empty so just 2)
            block_size.div_ceil(512),
            &[extent],
        )
    }

    /// Create the directory entry for `lost+found` to be placed in the parent (Root)
    pub fn entry() -> ExtDirEntry {
        ExtDirEntry::dir(Self::INODE, Self::NAME)
    }
}
