// SPDX-License-Identifier: MIT
//! EXT Directory Entry structure

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use zerocopy::byteorder::little_endian::{U16, U32};
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
#[repr(C)]
pub struct ExtDirEntryHeader {
    pub inode: U32,
    pub rec_len: U16,
    pub name_len: u8,
    pub file_type: u8,
}

impl ExtDirEntryHeader {
    /// Borrow the fixed header and name, bounded by this record's declared length.
    pub fn from_record(bytes: &[u8]) -> Option<(&Self, &[u8])> {
        let (header, _) = Self::ref_from_prefix(bytes).ok()?;
        let record = bytes.get(..header.rec_len.get() as usize)?;
        let name = record.get(8..8 + header.name_len as usize)?;
        Some((header, name))
    }
}

const _: () = {
    assert!(core::mem::size_of::<ExtDirEntryHeader>() == 8);
    assert!(core::mem::align_of::<ExtDirEntryHeader>() == 1);
    assert!(core::mem::offset_of!(ExtDirEntryHeader, rec_len) == 4);
    assert!(core::mem::offset_of!(ExtDirEntryHeader, name_len) == 6);
    assert!(core::mem::offset_of!(ExtDirEntryHeader, file_type) == 7);
};

/// EXT Directory Entry structure
///
/// This represents an on-disk directory entry for EXT filesystems.
/// The structure is variable-length with a minimum of 8 bytes header.
#[derive(Debug, Clone)]
pub struct ExtDirEntry {
    pub inode: u32,
    pub rec_len: u16,
    pub name_len: u8,
    pub file_type: u8,
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
            inode: self.inode.into(),
            rec_len: self.rec_len.into(),
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
        let (header, name) = ExtDirEntryHeader::from_record(data)?;
        let inode = header.inode.get();
        let rec_len = header.rec_len.get();
        let name_len = header.name_len;
        let file_type = header.file_type;
        let name = name.to_vec();

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

        // Pad to block_size and adjust last entry's rec_len
        crate::utils::pad_directory_block(&mut buf, block_size);

        buf
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

#[cfg(test)]
mod header_tests {
    use super::*;

    #[test]
    fn directory_header_is_little_endian_and_name_stays_in_record() {
        let bytes = [0x78, 0x56, 0x34, 0x12, 12, 0, 3, 1, b'a', b'b', b'c', 0];
        let (header, name) = ExtDirEntryHeader::from_record(&bytes).unwrap();
        assert_eq!(header.inode.get(), 0x12345678);
        assert_eq!(name, b"abc");
        assert_eq!(header.as_bytes(), &bytes[..8]);
        let mut encoded = Vec::new();
        ExtDirEntry::from_bytes(&bytes)
            .unwrap()
            .to_raw_buffer(&mut encoded);
        assert_eq!(encoded, bytes);
        for len in 0..bytes.len() {
            assert!(ExtDirEntryHeader::from_record(&bytes[..len]).is_none());
        }
        let mut crosses_record = bytes;
        crosses_record[4] = 8;
        assert!(ExtDirEntryHeader::from_record(&crosses_record).is_none());
    }
}
