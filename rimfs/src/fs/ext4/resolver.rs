// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::core::resolver::*;
use crate::core::utils::path_utils::*;
use crate::fs::ext4::attr::Ext4Mode;
use crate::fs::ext4::constant::*;
use crate::fs::ext4::types::{Ext4Extent, Ext4ExtentHeader};
use crate::fs::ext4::{group_layout::GroupLayout, meta::Ext4Meta};
use rimio::{RimIO, RimIOExt};
use zerocopy::FromBytes;

pub struct Ext4Resolver<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Ext4Meta,
}

impl<'a, IO: RimIO + ?Sized> Ext4Resolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Ext4Meta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: RimIO + ?Sized> Ext4Resolver<'a, IO> {
    /// Read an inode from disk given its inode number (1-based)
    pub(crate) fn read_inode(
        &mut self,
        inode_num: u32,
    ) -> FsResolverResult<[u8; EXT4_DEFAULT_INODE_SIZE as usize]> {
        if inode_num < 1 {
            return Err(FsResolverError::Invalid("Invalid inode number 0"));
        }

        let inode_index = inode_num - 1;
        let group = inode_index / self.meta.inodes_per_group;
        let index_in_group = inode_index % self.meta.inodes_per_group;

        let layout = GroupLayout::compute(self.meta, group);
        let inode_table_block = layout.inode_table_block;

        let inode_size = EXT4_DEFAULT_INODE_SIZE as u64;
        let offset = (inode_table_block as u64 * self.meta.block_size as u64)
            + (index_in_group as u64 * inode_size);

        let mut buf = [0u8; EXT4_DEFAULT_INODE_SIZE as usize];
        self.io
            .read_at(offset, &mut buf)
            .map_err(FsResolverError::IO)?;
        Ok(buf)
    }

    /// Parse inode mode to determine if it's a directory
    pub(crate) fn inode_is_dir(&self, inode_buf: &[u8]) -> bool {
        if let Some(chunk) = inode_buf.get(0..2)
            && let Ok(arr) = chunk.try_into()
        {
            let i_mode = u16::from_le_bytes(arr);
            return (i_mode & 0xF000) == Ext4Mode::DIR.bits();
        }
        false
    }

    /// Get inode size (lower 32 bits)
    pub(crate) fn inode_size(&self, inode_buf: &[u8]) -> u32 {
        if let Some(chunk) = inode_buf.get(4..8)
            && let Ok(arr) = chunk.try_into()
        {
            return u32::from_le_bytes(arr);
        }
        0
    }

    /// Read extents from inode buffer
    pub(crate) fn read_extents(&self, inode_buf: &[u8]) -> FsResolverResult<Vec<Ext4Extent>> {
        // Check inode uses extents
        let i_flags = inode_buf
            .get(32..36)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);
        if i_flags & EXT4_INODE_FLAG_EXTENTS == 0 {
            return Err(FsResolverError::Invalid(
                "Inode does not use extents (block map not supported)",
            ));
        }

        // Extent header is at offset 40 in inode
        let header = Ext4ExtentHeader::read_from_bytes(&inode_buf[40..52])
            .ok()
            .ok_or(FsResolverError::Invalid("Failed to read extent header"))?;

        if header.eh_magic != EXT4_EXTENT_HEADER_MAGIC {
            return Err(FsResolverError::Invalid("Invalid extent header magic"));
        }

        if header.eh_depth != 0 {
            // TODO: support extent tree traversal
            return Err(FsResolverError::Other(
                "Extent tree (depth > 0) not yet supported",
            ));
        }

        let entries_count = header.eh_entries as usize;
        let mut extents = Vec::with_capacity(entries_count);

        // Extents start at offset 52 (40 + 12)
        for i in 0..entries_count.min(4) {
            let offset = 52 + i * 12;
            if let Ok(extent) = Ext4Extent::read_from_bytes(&inode_buf[offset..offset + 12]) {
                extents.push(extent);
            }
        }

        Ok(extents)
    }

    /// Read file content given inode number
    fn read_file_content(&mut self, inode_num: u32) -> FsResolverResult<Vec<u8>> {
        let inode_buf = self.read_inode(inode_num)?;
        let size = self.inode_size(&inode_buf) as usize;

        if size == 0 {
            return Ok(Vec::new());
        }

        let extents = self.read_extents(&inode_buf)?;
        // Collect all block offsets
        let mut offsets = Vec::new();
        let block_size = self.meta.block_size as usize;
        let blocks_needed = size.div_ceil(block_size);

        // Populate offsets from extents
        for extent in &extents {
            if offsets.len() >= blocks_needed {
                break;
            }
            let phys_block = extent.ee_start_lo;
            let len_blocks = extent.ee_len as usize;
            for blk_idx in 0..len_blocks {
                if offsets.len() >= blocks_needed {
                    break;
                }
                let blk = phys_block + blk_idx as u32;
                offsets.push(blk as u64 * block_size as u64);
            }
        }

        let mut out = vec![0u8; size];
        if offsets.is_empty() {
            return Ok(out);
        }

        let full_blocks = size / block_size;
        let partial_bytes = size % block_size;

        // Read full blocks
        if full_blocks > 0 {
            // Take first 'full_blocks' offsets
            let full_offsets = &offsets[..full_blocks];
            let full_len = full_blocks * block_size;
            self.io
                .read_multi_at(full_offsets, block_size, &mut out[..full_len])
                .map_err(FsResolverError::IO)?;
        }

        // Read tail if exists
        if partial_bytes > 0
            && let Some(&last_offset) = offsets.get(full_blocks)
        {
            let start = full_blocks * block_size;
            self.io
                .read_at(last_offset, &mut out[start..])
                .map_err(FsResolverError::IO)?;
        }

        Ok(out)
    }

    /// Read directory entries from a directory inode
    pub(crate) fn read_dir_entries(
        &mut self,
        dir_inode: u32,
    ) -> FsResolverResult<Vec<Ext4DirEntry>> {
        let inode_buf = self.read_inode(dir_inode)?;

        if !self.inode_is_dir(&inode_buf) {
            return Err(FsResolverError::Invalid("Not a directory"));
        }

        let dir_size = self.inode_size(&inode_buf) as usize;
        let extents = self.read_extents(&inode_buf)?;
        let block_size = self.meta.block_size as usize;

        let mut offsets = Vec::new();
        let blocks_needed = dir_size.div_ceil(block_size);

        for extent in &extents {
            let phys_block = extent.ee_start_lo;
            let len_blocks = extent.ee_len as usize;
            for blk_idx in 0..len_blocks {
                if offsets.len() >= blocks_needed {
                    break;
                }
                let blk = phys_block + blk_idx as u32;
                offsets.push(blk as u64 * block_size as u64);
            }
        }

        let mut raw_data = vec![0u8; offsets.len() * block_size];
        if !offsets.is_empty() {
            self.io
                .read_multi_at(&offsets, block_size, &mut raw_data)
                .map_err(FsResolverError::IO)?;
        }

        // Iterate over raw bytes to parse entries
        let mut entries = Vec::new();
        let mut total_read = 0usize;

        // We iterate block by block from raw_data
        for chunk in raw_data.chunks(block_size) {
            if total_read >= dir_size {
                break;
            }

            let buf = chunk; // Already read
            let mut pos = 0usize;
            while pos + 8 <= buf.len() && total_read + pos < dir_size {
                let entry_inode_bytes: [u8; 4] = buf[pos..pos + 4].try_into().unwrap_or([0; 4]);
                let entry_inode = u32::from_le_bytes(entry_inode_bytes);

                let rec_len_bytes: [u8; 2] = buf[pos + 4..pos + 6].try_into().unwrap_or([0; 2]);
                let rec_len = u16::from_le_bytes(rec_len_bytes) as usize;

                let name_len = buf[pos + 6] as usize;
                let file_type = buf[pos + 7];

                if rec_len == 0 || rec_len > buf.len() - pos {
                    break; // End of directory or corrupt entry
                }

                if entry_inode != 0 && name_len > 0 && pos + 8 + name_len <= buf.len() {
                    let name_bytes = &buf[pos + 8..pos + 8 + name_len];
                    if let Ok(name) = core::str::from_utf8(name_bytes) {
                        // Skip . and ..
                        if name != "." && name != ".." {
                            entries.push(Ext4DirEntry {
                                inode: entry_inode,
                                name: name.to_string(),
                                file_type,
                            });
                        }
                    }
                }

                pos += rec_len;
            }
            total_read += block_size;
        }

        // Sort entries by name (case-insensitive, like fat32/exfat)
        entries.sort_by(|a, b| {
            a.name
                .bytes()
                .map(|c| c.to_ascii_lowercase())
                .cmp(b.name.bytes().map(|c| c.to_ascii_lowercase()))
        });

        Ok(entries)
    }

    /// Find an entry by name in a directory
    fn find_in_dir(
        &mut self,
        dir_inode: u32,
        name: &str,
    ) -> FsResolverResult<Option<Ext4DirEntry>> {
        let entries = self.read_dir_entries(dir_inode)?;
        let target_lower: Vec<u8> = name.bytes().map(|c| c.to_ascii_lowercase()).collect();

        for entry in entries {
            let entry_lower: Vec<u8> = entry.name.bytes().map(|c| c.to_ascii_lowercase()).collect();
            if entry_lower == target_lower {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }
}

/// A parsed directory entry
#[derive(Debug, Clone)]
pub struct Ext4DirEntry {
    pub(crate) inode: u32,
    pub(crate) name: String,
    pub(crate) file_type: u8,
}

impl Ext4DirEntry {
    fn is_dir(&self) -> bool {
        self.file_type == EXT4_FT_DIR
    }
}

impl<'a, IO: RimIO + ?Sized> FsResolver for Ext4Resolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, inode, _) = self.resolve_path(path)?;
        if !is_dir {
            return Err(FsResolverError::Invalid("Not a directory"));
        }

        let entries = self.read_dir_entries(inode)?;
        Ok(entries.into_iter().map(|e| e.name).collect())
    }

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let (is_dir, inode, _) = self.resolve_path(path)?;
        if is_dir {
            return Err(FsResolverError::Invalid("Not a file"));
        }

        self.read_file_content(inode)
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        if path.is_empty() || path == "/" {
            return Ok(FileAttributes::new_dir());
        }

        let components = split_path(path);
        let mut current_inode = EXT4_ROOT_INODE;

        for (i, comp) in components.iter().enumerate() {
            let entry = self
                .find_in_dir(current_inode, comp)?
                .ok_or(FsResolverError::NotFound)?;

            if i == components.len() - 1 {
                // Last component: read inode for attributes
                let inode_buf = self.read_inode(entry.inode)?;
                return Ok(self.parse_attributes(&inode_buf, entry.is_dir()));
            }

            if !entry.is_dir() {
                return Err(FsResolverError::Invalid(
                    "Expected directory for intermediate component",
                ));
            }
            current_inode = entry.inode;
        }

        Err(FsResolverError::Invalid("Invalid path"))
    }

    fn resolve_path(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        if path.is_empty() || path == "/" {
            return Ok((true, EXT4_ROOT_INODE, 0));
        }

        let components = split_path(path);
        let mut current_inode = EXT4_ROOT_INODE;

        for (i, comp) in components.iter().enumerate() {
            let entry = self
                .find_in_dir(current_inode, comp)?
                .ok_or(FsResolverError::NotFound)?;

            let is_last = i == components.len() - 1;
            let is_dir = entry.is_dir();

            if is_last {
                // Get size from inode
                let inode_buf = self.read_inode(entry.inode)?;
                let size = self.inode_size(&inode_buf) as usize;
                return Ok((is_dir, entry.inode, size));
            }

            if !is_dir {
                return Err(FsResolverError::Invalid("Expected a directory"));
            }

            current_inode = entry.inode;
        }

        Err(FsResolverError::Invalid("Invalid path"))
    }
}

impl<'a, IO: RimIO + ?Sized> Ext4Resolver<'a, IO> {
    /// Parse file attributes from inode buffer
    fn parse_attributes(&self, inode_buf: &[u8], is_dir: bool) -> FileAttributes {
        let i_mode = inode_buf
            .get(0..2)
            .and_then(|b| b.try_into().ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0);

        let i_atime = inode_buf
            .get(8..12)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);

        let i_ctime = inode_buf
            .get(12..16)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);

        let i_mtime = inode_buf
            .get(16..20)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);

        // Convert i_mode permissions to mode bits
        let mode = (i_mode & 0x0FFF) as u32;

        let mut attr = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };

        attr.mode = Some(mode);

        // Try to convert timestamps
        if let Ok(atime) = time::OffsetDateTime::from_unix_timestamp(i_atime as i64) {
            attr.accessed = Some(atime);
        }
        if let Ok(ctime) = time::OffsetDateTime::from_unix_timestamp(i_ctime as i64) {
            attr.created = Some(ctime);
        }
        if let Ok(mtime) = time::OffsetDateTime::from_unix_timestamp(i_mtime as i64) {
            attr.modified = Some(mtime);
        }

        attr
    }
}
