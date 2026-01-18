// SPDX-License-Identifier: MIT
//! EXT4 Directory Entry structure

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use crate::fs::ext4::constant::*;

/// EXT4 Directory Entry structure
///
/// This represents an on-disk directory entry for EXT4 filesystems.
/// The structure is variable-length with a minimum of 8 bytes header.
#[derive(Debug, Clone)]
pub struct Ext4DirEntry {
    /// Inode number
    pub inode: u32,
    /// Record length (total size of this entry including padding)
    pub rec_len: u16,
    /// Name length (excluding null terminator)
    pub name_len: u8,
    /// File type (EXT4_FT_* constants)
    pub file_type: u8,
    /// Entry name (variable length)
    pub name: Vec<u8>,
}

impl Ext4DirEntry {
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
        Self::new(current_inode, ".", EXT4_FT_DIR)
    }

    /// Create a ".." entry for a directory
    pub fn dotdot(parent_inode: u32) -> Self {
        Self::new(parent_inode, "..", EXT4_FT_DIR)
    }

    /// Create an entry for a subdirectory
    pub fn dir(inode: u32, name: &str) -> Self {
        Self::new(inode, name, EXT4_FT_DIR)
    }

    /// Create an entry for a regular file
    pub fn file(inode: u32, name: &str) -> Self {
        Self::new(inode, name, EXT4_FT_REG_FILE)
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
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.rec_len as usize);

        // inode (4 bytes)
        buf.extend_from_slice(&self.inode.to_le_bytes());

        // rec_len (2 bytes)
        buf.extend_from_slice(&self.rec_len.to_le_bytes());

        // name_len (1 byte)
        buf.push(self.name_len);

        // file_type (1 byte)
        buf.push(self.file_type);

        // name (variable)
        buf.extend_from_slice(&self.name);

        // Padding to rec_len
        while buf.len() < self.rec_len as usize {
            buf.push(0);
        }

        buf
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
        self.file_type == EXT4_FT_DIR
    }

    /// Check if this is a file entry
    pub fn is_file(&self) -> bool {
        self.file_type == EXT4_FT_REG_FILE
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
