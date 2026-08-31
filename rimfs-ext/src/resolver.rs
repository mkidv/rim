// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::constant::*;
use crate::core::resolver::*;
use crate::core::traits::FsMeta;
use crate::core::utils::path_utils::*;
use crate::types::{BlockMapArray, ExtExtent, ExtExtentHeader, ExtExtentIndex};
use crate::{group_layout::GroupLayout, meta::ExtMeta};
use rimio::prelude::*;
use zerocopy::FromBytes;

pub struct ExtResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExtMeta,
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExtMeta) -> Self {
        Self { io, meta }
    }
}

use crate::core::resolver::walker::WalkerDataSource;

impl<'a, IO: RimRead + ?Sized> WalkerDataSource for ExtResolver<'a, IO> {
    type Entry = ExtDirEntry;

    fn root_cluster(&self) -> u32 {
        EXT_ROOT_INODE
    }

    fn find_entry(&mut self, dir_inode: u32, name: &str) -> FsResolverResult<Option<Self::Entry>> {
        self.find_in_dir(dir_inode, name)
    }

    fn is_dir(&self, entry: &Self::Entry) -> bool {
        entry.is_dir()
    }

    fn entry_cluster(&self, entry: &Self::Entry) -> u32 {
        entry.inode
    }
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    /// Read inode raw bytes from inode table
    pub(crate) fn read_inode(&mut self, inode_num: u32) -> FsResolverResult<Vec<u8>> {
        if inode_num == 0 || inode_num as u64 > self.meta.inode_count {
            return Err(FsResolverError::Invalid("Invalid inode number"));
        }

        let inode_index = inode_num - 1;
        let group = inode_index / self.meta.inodes_per_group;
        let index_in_group = inode_index % self.meta.inodes_per_group;

        let layout = GroupLayout::compute(self.meta, group);
        let inode_table_block = layout.inode_table_block;

        let inode_size = self.meta.inode_size as u64;
        let offset = (inode_table_block * self.meta.block_size as u64)
            + (index_in_group as u64 * inode_size);

        let mut buf = vec![0u8; inode_size as usize];
        self.io
            .read_at(offset, &mut buf)
            .map_err(FsResolverError::IO)?;
        Ok(buf)
    }

    /// Check if inode is a directory
    fn inode_is_dir(&self, inode_buf: &[u8]) -> bool {
        let mode = u16::from_le_bytes(inode_buf[0..2].try_into().unwrap_or([0; 2]));
        (mode & 0xF000) == 0x4000
    }

    /// Get size of file from inode
    fn inode_size(&self, inode_buf: &[u8]) -> u64 {
        let size_lo = u32::from_le_bytes(inode_buf[4..8].try_into().unwrap_or([0; 4])) as u64;
        let size_hi = if inode_buf.len() >= 112 {
            u32::from_le_bytes(inode_buf[108..112].try_into().unwrap_or([0; 4])) as u64
        } else {
            0
        };
        (size_hi << 32) | size_lo
    }

    /// Recursively collect leaf extents from an extent node
    fn collect_extents_from_node(
        &mut self,
        header: &ExtExtentHeader,
        entries_buf: &[u8],
        max_depth: u16,
        out: &mut Vec<ExtExtent>,
    ) -> FsResolverResult<()> {
        if header.eh_magic != EXT_EXTENT_HEADER_MAGIC {
            return Err(FsResolverError::Invalid("Invalid extent header magic"));
        }

        let entries_count = header.eh_entries as usize;

        if header.eh_depth == 0 {
            // Leaf node: entries are ExtExtent
            for i in 0..entries_count {
                let offset = i * core::mem::size_of::<ExtExtent>();
                if offset + core::mem::size_of::<ExtExtent>() <= entries_buf.len()
                    && let Ok(extent) = ExtExtent::read_from_bytes(
                        &entries_buf[offset..offset + core::mem::size_of::<ExtExtent>()],
                    )
                {
                    out.push(extent);
                }
            }
        } else {
            // Index node: entries are ExtExtentIndex
            if max_depth == 0 {
                return Err(FsResolverError::Invalid("Extent tree depth exceeded limit"));
            }

            let block_size = self.meta.block_size as usize;
            let mut child_buf = vec![0u8; block_size];

            for i in 0..entries_count {
                let offset = i * core::mem::size_of::<ExtExtentIndex>();
                if offset + core::mem::size_of::<ExtExtentIndex>() <= entries_buf.len()
                    && let Ok(index_entry) = ExtExtentIndex::read_from_bytes(
                        &entries_buf[offset..offset + core::mem::size_of::<ExtExtentIndex>()],
                    )
                {
                    let child_block = index_entry.leaf_physical_block();
                    let child_offset = child_block * (self.meta.block_size as u64);
                    self.io
                        .read_at(child_offset, &mut child_buf)
                        .map_err(FsResolverError::IO)?;

                    let child_header = ExtExtentHeader::read_from_bytes(&child_buf[0..12])
                        .ok()
                        .ok_or(FsResolverError::Invalid(
                            "Failed to read child extent header",
                        ))?;

                    self.collect_extents_from_node(
                        &child_header,
                        &child_buf[12..],
                        max_depth - 1,
                        out,
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Read extents from inode buffer
    pub(crate) fn read_extents(&mut self, inode_buf: &[u8]) -> FsResolverResult<Vec<ExtExtent>> {
        // Check inode uses extents
        let i_flags = inode_buf
            .get(32..36)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);
        if i_flags & EXT_INODE_FLAG_EXTENTS == 0 {
            return Err(FsResolverError::Invalid(
                "Inode does not use extents (block map not supported)",
            ));
        }

        // Extent header is at offset 40 in inode
        let header = ExtExtentHeader::read_from_bytes(&inode_buf[40..52])
            .ok()
            .ok_or(FsResolverError::Invalid("Failed to read extent header"))?;

        if header.eh_magic != EXT_EXTENT_HEADER_MAGIC {
            return Err(FsResolverError::Invalid("Invalid extent header magic"));
        }

        let mut extents = Vec::new();
        // Inode extent buffer is at offset 52..100 (48 bytes max in 128/256-byte inode header)
        let entries_slice = inode_buf.get(52..100).unwrap_or(&inode_buf[52..]);
        self.collect_extents_from_node(&header, entries_slice, 5, &mut extents)?;

        Ok(extents)
    }

    /// Read blocks from Block Map (Ext2/3)
    fn read_block_map(&mut self, inode_buf: &[u8], size: usize) -> FsResolverResult<Vec<u32>> {
        let map = BlockMapArray::read_from_bytes(&inode_buf[40..100])
            .map_err(|_| FsResolverError::Invalid("Failed to read block map"))?;

        // We need to collect all blocks up to `size`.
        let block_size = self.meta.block_size as usize;
        let blocks_count = size.div_ceil(block_size);
        let mut blocks = Vec::with_capacity(blocks_count);

        // 1. Direct Blocks
        for &blk in &map.direct {
            if blocks.len() >= blocks_count {
                break;
            }
            blocks.push(blk);
        }

        if blocks.len() >= blocks_count {
            return Ok(blocks);
        }

        // 2. Indirect Block
        if map.indirect != 0 {
            self.read_indirect_block(map.indirect, &mut blocks, blocks_count)?;
        }

        if blocks.len() >= blocks_count {
            return Ok(blocks);
        }

        // 3. Double Indirect
        if map.double_indirect != 0 {
            // Read the double indirect block, which contains pointers to indirect blocks
            let mut indirects = Vec::new();
            self.read_indirect_block(map.double_indirect, &mut indirects, usize::MAX)?; // Read all ptrs

            for &indirect_blk in &indirects {
                if blocks.len() >= blocks_count {
                    break;
                }
                self.read_indirect_block(indirect_blk, &mut blocks, blocks_count)?;
            }
        }

        // 4. Triple Indirect
        if map.triple_indirect != 0 && blocks.len() < blocks_count {
            let mut double_indirects = Vec::new();
            self.read_indirect_block(map.triple_indirect, &mut double_indirects, usize::MAX)?;

            for &double_blk in &double_indirects {
                if blocks.len() >= blocks_count {
                    break;
                }
                let mut indirects = Vec::new();
                self.read_indirect_block(double_blk, &mut indirects, usize::MAX)?;

                for &indirect_blk in &indirects {
                    if blocks.len() >= blocks_count {
                        break;
                    }
                    self.read_indirect_block(indirect_blk, &mut blocks, blocks_count)?;
                }
            }
        }

        Ok(blocks)
    }

    fn read_indirect_block(
        &mut self,
        block: u32,
        blocks: &mut Vec<u32>,
        limit: usize,
    ) -> FsResolverResult<()> {
        if block == 0 {
            return Ok(());
        }
        let offset = self.meta.unit_offset(block);
        let mut buf = vec![0u8; self.meta.block_size as usize];
        self.io
            .read_at(offset, &mut buf)
            .map_err(FsResolverError::IO)?;

        for chunk in buf.chunks(4) {
            if blocks.len() >= limit {
                break;
            }
            if let Ok(bytes) = chunk.try_into() {
                let blk = u32::from_le_bytes(bytes);
                blocks.push(blk);
            }
        }
        Ok(())
    }

    /// Read file content given inode number
    fn read_file_content(&mut self, inode_num: u32) -> FsResolverResult<Vec<u8>> {
        let inode_buf = self.read_inode(inode_num)?;
        let size = self.inode_size(&inode_buf) as usize;

        if size == 0 {
            return Ok(Vec::new());
        }

        let mut block_offsets: Vec<Option<u64>> = Vec::new();
        let block_size = self.meta.block_size as usize;
        let blocks_needed = size.div_ceil(block_size);

        // Check if using extents
        let i_flags = inode_buf
            .get(32..36)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);

        if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
            let extents = self.read_extents(&inode_buf)?;
            for extent in &extents {
                if block_offsets.len() >= blocks_needed {
                    break;
                }
                let len_blocks = extent.len() as usize;
                let is_uninit = extent.is_uninit();
                let phys_block = extent.physical_start();

                for blk_idx in 0..len_blocks {
                    if block_offsets.len() >= blocks_needed {
                        break;
                    }
                    if is_uninit {
                        block_offsets.push(None);
                    } else {
                        let blk = phys_block + blk_idx as u64;
                        block_offsets.push(Some(blk * block_size as u64));
                    }
                }
            }
        } else {
            // Block Map
            let blocks = self.read_block_map(&inode_buf, size)?;
            for blk in blocks {
                if blk == 0 {
                    block_offsets.push(None);
                } else {
                    block_offsets.push(Some(blk as u64 * block_size as u64));
                }
            }
        }

        let mut out = vec![0u8; size];
        for (i, opt_offset) in block_offsets.iter().enumerate() {
            let start = i * block_size;
            if start >= size {
                break;
            }
            let end = (start + block_size).min(size);
            if let Some(offset) = opt_offset {
                self.io
                    .read_at(*offset, &mut out[start..end])
                    .map_err(FsResolverError::IO)?;
            }
        }

        Ok(out)
    }

    /// Read directory entries from a directory inode
    pub(crate) fn read_dir_entries(
        &mut self,
        dir_inode: u32,
    ) -> FsResolverResult<Vec<ExtDirEntry>> {
        let inode_buf = self.read_inode(dir_inode)?;

        if !self.inode_is_dir(&inode_buf) {
            return Err(FsResolverError::Invalid("Not a directory"));
        }

        let dir_size = self.inode_size(&inode_buf) as usize;
        let block_size = self.meta.block_size as usize;
        let mut offsets = Vec::new();
        let blocks_needed = dir_size.div_ceil(block_size);

        // Check flags again (duplicated logic, could be helper)
        let i_flags = inode_buf
            .get(32..36)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);

        if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
            let extents = self.read_extents(&inode_buf)?;
            for extent in &extents {
                let phys_block = extent.physical_start();
                let len_blocks = (extent.ee_len & 0x7FFF) as usize;
                for blk_idx in 0..len_blocks {
                    if offsets.len() >= blocks_needed {
                        break;
                    }
                    let blk = phys_block + blk_idx as u64;
                    offsets.push(blk * block_size as u64);
                }
            }
        } else {
            // Block Map
            let blocks = self.read_block_map(&inode_buf, dir_size)?;
            for blk in blocks {
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
                            entries.push(ExtDirEntry {
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
    fn find_in_dir(&mut self, dir_inode: u32, name: &str) -> FsResolverResult<Option<ExtDirEntry>> {
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
pub struct ExtDirEntry {
    pub(crate) inode: u32,
    pub(crate) name: String,
    pub(crate) file_type: u8,
}

impl ExtDirEntry {
    pub fn is_dir(&self) -> bool {
        self.file_type == EXT_FT_DIR
    }

    pub fn is_file(&self) -> bool {
        self.file_type == EXT_FT_REG_FILE
    }

    pub fn is_symlink(&self) -> bool {
        self.file_type == EXT_FT_SYMLINK
    }
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    pub fn resolve_entry_info(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        match crate::core::resolver::walker::walk_path(self, path)? {
            Some(entry) => {
                // Get size from inode
                let inode_buf = self.read_inode(entry.inode)?;
                let size = self.inode_size(&inode_buf) as usize;
                Ok((entry.is_dir(), entry.inode, size))
            }
            None => Ok((true, EXT_ROOT_INODE, 0)),
        }
    }
    pub(crate) fn inode_extents(
        &mut self,
        inode_buf: &[u8],
        total_size: u64,
    ) -> FsResolverResult<Vec<rimio::extent::IoExtent>> {
        let block_size = self.meta.block_size as u64;
        let mut fs_extents = Vec::new();

        let i_flags = inode_buf
            .get(32..36)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes)
            .unwrap_or(0);

        if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
            let extents = self.read_extents(inode_buf)?;
            for ext in extents {
                let logical_offset = ext.ee_block as u64 * block_size;
                if logical_offset >= total_size {
                    break;
                }
                let byte_len = ext.len() as u64 * block_size;
                let actual_len = core::cmp::min(byte_len, total_size - logical_offset);
                let source_offset = if ext.is_uninit() {
                    None
                } else {
                    Some(ext.physical_start() * block_size)
                };
                fs_extents.push(rimio::extent::IoExtent {
                    logical_offset,
                    source_offset,
                    len: actual_len,
                });
            }
        } else {
            let blocks = self.read_block_map(inode_buf, total_size as usize)?;
            let mut logical_offset = 0u64;
            for blk in blocks {
                if logical_offset >= total_size {
                    break;
                }
                let extent_len = core::cmp::min(block_size, total_size - logical_offset);
                let source_offset = if blk == 0 {
                    None
                } else {
                    Some(blk as u64 * block_size)
                };
                fs_extents.push(rimio::extent::IoExtent {
                    logical_offset,
                    source_offset,
                    len: extent_len,
                });
                logical_offset += extent_len;
            }
        }

        Ok(fs_extents)
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for ExtResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, inode, _) = self.resolve_entry_info(path)?;
        crate::ensure!(is_dir, FsResolverError::Invalid("Not a directory"));

        let entries = self.read_dir_entries(inode)?;
        Ok(entries.into_iter().map(|e| e.name).collect())
    }

    fn open_file<'c>(
        &'c mut self,
        path: &str,
    ) -> FsResolverResult<alloc::boxed::Box<dyn rimio::RimRead + 'c>> {
        let (is_dir, inode, size) = self.resolve_entry_info(path)?;
        crate::ensure!(!is_dir, FsResolverError::Invalid("Not a file"));
        if size == 0 {
            return Ok(alloc::boxed::Box::new(rimio::SliceRimIO::new(&[])));
        }

        let inode_buf = self.read_inode(inode)?;
        let total_size = size as u64;
        let extents = self.inode_extents(&inode_buf, total_size)?;

        Ok(alloc::boxed::Box::new(rimio::extent::ExtentRimRead::new(
            &mut *self.io,
            extents,
            total_size,
        )))
    }

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let (is_dir, inode, _) = self.resolve_entry_info(path)?;
        crate::ensure!(!is_dir, FsResolverError::Invalid("Not a file"));

        self.read_file_content(inode)
    }

    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        let components = split_path(path);
        let mut current_inode = EXT_ROOT_INODE;

        for (i, comp) in components.iter().enumerate() {
            let entry = self
                .find_in_dir(current_inode, comp)?
                .ok_or(FsResolverError::NotFound)?;

            if i == components.len() - 1 {
                let inode_buf = self.read_inode(entry.inode)?;
                let i_mode = inode_buf
                    .get(0..2)
                    .and_then(|b| b.try_into().ok())
                    .map(u16::from_le_bytes)
                    .unwrap_or(0);

                crate::ensure!(
                    (i_mode & 0xF000) == 0xA000,
                    FsResolverError::Invalid("Not a symlink")
                );

                let size = self.inode_size(&inode_buf) as usize;
                let i_blocks = inode_buf
                    .get(28..32)
                    .and_then(|b| b.try_into().ok())
                    .map(u32::from_le_bytes)
                    .unwrap_or(0);

                if i_blocks == 0 && size < 60 {
                    // Fast symlink
                    let target_bytes =
                        inode_buf
                            .get(40..40 + size)
                            .ok_or(FsResolverError::Invalid(
                                "Inode buffer too small for fast symlink",
                            ))?;
                    return String::from_utf8(target_bytes.to_vec())
                        .map_err(|_| FsResolverError::Invalid("Invalid UTF-8 in symlink target"));
                } else {
                    // Slow symlink
                    let content = self.read_file_content(entry.inode)?;
                    let target_slice = if content.len() >= size {
                        &content[..size]
                    } else {
                        &content[..]
                    };
                    return String::from_utf8(target_slice.to_vec())
                        .map_err(|_| FsResolverError::Invalid("Invalid UTF-8 in symlink target"));
                }
            }

            if !entry.is_dir() {
                return Err(FsResolverError::Invalid(
                    "Expected directory for intermediate component",
                ));
            }
            current_inode = entry.inode;
        }

        Err(FsResolverError::NotFound)
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        if path.is_empty() || path == "/" {
            return Ok(FileAttributes::new_dir());
        }

        let components = split_path(path);
        let mut current_inode = EXT_ROOT_INODE;

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
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    /// Parse file attributes from inode buffer
    fn parse_attributes(&self, inode_buf: &[u8], is_dir: bool) -> FileAttributes {
        let i_mode = inode_buf
            .get(0..2)
            .and_then(|b| b.try_into().ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0);

        let i_uid_lo = inode_buf
            .get(2..4)
            .and_then(|b| b.try_into().ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0) as u32;

        let i_gid_lo = inode_buf
            .get(24..26)
            .and_then(|b| b.try_into().ok())
            .map(u16::from_le_bytes)
            .unwrap_or(0) as u32;

        let (uid_hi, gid_hi) = if inode_buf.len() >= 128 {
            let u_hi = inode_buf
                .get(120..122)
                .and_then(|b| b.try_into().ok())
                .map(u16::from_le_bytes)
                .unwrap_or(0) as u32;
            let g_hi = inode_buf
                .get(122..124)
                .and_then(|b| b.try_into().ok())
                .map(u16::from_le_bytes)
                .unwrap_or(0) as u32;
            (u_hi, g_hi)
        } else {
            (0, 0)
        };

        let uid = (uid_hi << 16) | i_uid_lo;
        let gid = (gid_hi << 16) | i_gid_lo;

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

        let kind = match i_mode & 0xF000 {
            0x4000 => crate::core::traits::NodeKind::Directory,
            0xA000 => crate::core::traits::NodeKind::Symlink,
            0x1000 => crate::core::traits::NodeKind::Fifo,
            0xC000 => crate::core::traits::NodeKind::Socket,
            0x2000 => crate::core::traits::NodeKind::CharDevice,
            0x6000 => crate::core::traits::NodeKind::BlockDevice,
            _ => {
                if is_dir {
                    crate::core::traits::NodeKind::Directory
                } else {
                    crate::core::traits::NodeKind::Regular
                }
            }
        };

        let mode = (i_mode & 0x0FFF) as u32;

        let mut attr = match kind {
            crate::core::traits::NodeKind::Directory => FileAttributes::new_dir(),
            crate::core::traits::NodeKind::Symlink => FileAttributes::new_symlink(),
            _ => FileAttributes::new_file(),
        };

        attr.kind = kind;
        attr.mode = Some(mode);
        attr.uid = Some(uid);
        attr.gid = Some(gid);

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

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::IntoBytes;

    #[test]
    fn test_extent_tree_depth_0() {
        let meta = ExtMeta::new(64 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 64 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);
        let mut resolver = ExtResolver::new(&mut io, &meta);

        let mut inode = [0u8; 256];
        inode[32..36].copy_from_slice(&EXT_INODE_FLAG_EXTENTS.to_le_bytes());

        let header = ExtExtentHeader {
            eh_magic: EXT_EXTENT_HEADER_MAGIC,
            eh_entries: 2,
            eh_max: 4,
            eh_depth: 0,
            eh_generation: 0,
        };
        inode[40..52].copy_from_slice(header.as_bytes());

        let ext1 = ExtExtent::new_48(0, 100, 5);
        let ext2 = ExtExtent::new_48(5, 200, 10);
        inode[52..64].copy_from_slice(ext1.as_bytes());
        inode[64..76].copy_from_slice(ext2.as_bytes());

        let extents = resolver.read_extents(&inode).unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].physical_start(), 100);
        assert_eq!({ extents[0].ee_len }, 5);
        assert_eq!(extents[1].physical_start(), 200);
        assert_eq!({ extents[1].ee_len }, 10);
    }

    #[test]
    fn test_extent_tree_depth_1() {
        let meta = ExtMeta::new(64 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 64 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);

        // Child block at block 500
        let child_block_num = 500u64;
        let child_offset = child_block_num * 4096;

        let child_header = ExtExtentHeader {
            eh_magic: EXT_EXTENT_HEADER_MAGIC,
            eh_entries: 2,
            eh_max: 340,
            eh_depth: 0,
            eh_generation: 0,
        };
        let ext1 = ExtExtent::new_48(0, 1000, 20);
        let ext2 = ExtExtent::new_48(20, 2000, 30);

        let mut child_block_data = vec![0u8; 4096];
        child_block_data[0..12].copy_from_slice(child_header.as_bytes());
        child_block_data[12..24].copy_from_slice(ext1.as_bytes());
        child_block_data[24..36].copy_from_slice(ext2.as_bytes());
        io.write_at(child_offset, &child_block_data).unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);

        let mut inode = [0u8; 256];
        inode[32..36].copy_from_slice(&EXT_INODE_FLAG_EXTENTS.to_le_bytes());

        let root_header = ExtExtentHeader {
            eh_magic: EXT_EXTENT_HEADER_MAGIC,
            eh_entries: 1,
            eh_max: 4,
            eh_depth: 1,
            eh_generation: 0,
        };
        inode[40..52].copy_from_slice(root_header.as_bytes());

        let index_entry = ExtExtentIndex::new_48(0, child_block_num);
        inode[52..64].copy_from_slice(index_entry.as_bytes());

        let extents = resolver.read_extents(&inode).unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].physical_start(), 1000);
        assert_eq!({ extents[0].ee_len }, 20);
        assert_eq!(extents[1].physical_start(), 2000);
        assert_eq!({ extents[1].ee_len }, 30);
    }

    #[test]
    fn test_48bit_physical_extent() {
        let large_phys = 0x1_0000_2000u64;
        let ext = ExtExtent::new_48(0, large_phys, 8);
        assert_eq!(ext.physical_start(), large_phys);
        assert_eq!({ ext.ee_start_hi }, 1);
        assert_eq!({ ext.ee_start_lo }, 0x2000);
    }

    #[test]
    fn test_uninit_extent_zero_fill() {
        let meta = ExtMeta::new(32 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0xAAu8; 32 * 1024 * 1024]; // Pre-fill disk with garbage
        let mut io = MemRimIO::new(&mut disk);

        let mut inode = [0u8; 256];
        // File size = 8192 (2 blocks)
        inode[4..8].copy_from_slice(&8192u32.to_le_bytes());
        inode[32..36].copy_from_slice(&EXT_INODE_FLAG_EXTENTS.to_le_bytes());

        let header = ExtExtentHeader {
            eh_magic: EXT_EXTENT_HEADER_MAGIC,
            eh_entries: 1,
            eh_max: 4,
            eh_depth: 0,
            eh_generation: 0,
        };
        inode[40..52].copy_from_slice(header.as_bytes());

        // Uninitialized extent of 2 blocks (ee_len = 2 | 0x8000 = 0x8002 = 32770)
        let uninit_ext = ExtExtent::new_48(0, 50, 0x8002);
        assert!(uninit_ext.is_uninit());
        assert_eq!(uninit_ext.len(), 2);
        inode[52..64].copy_from_slice(uninit_ext.as_bytes());

        // Write inode into group 0 inode table (inode 12)
        let layout = GroupLayout::compute(&meta, 0);
        let inode_offset = (layout.inode_table_block * 4096) + (11 * 256);
        io.write_at(inode_offset, &inode).unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);
        let content = resolver.read_file_content(12).unwrap();
        assert_eq!(content.len(), 8192);
        assert!(content.iter().all(|&b| b == 0)); // Must be all zeroes despite disk being 0xAA
    }

    #[test]
    fn test_triple_indirect_block_map() {
        let meta = ExtMeta::new_ext2(64 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 64 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);

        // Triple indirect setup:
        // L3 block at 100 -> contains ptr to L2 block 200
        // L2 block at 200 -> contains ptr to L1 block 300
        // L1 block at 300 -> contains ptr to data block 400
        // Data block at 400 -> contains test data
        io.write_at(100 * 4096, &200u32.to_le_bytes()).unwrap();
        io.write_at(200 * 4096, &300u32.to_le_bytes()).unwrap();
        io.write_at(300 * 4096, &400u32.to_le_bytes()).unwrap();
        io.write_at(400 * 4096, b"TRIPLE_INDIRECT_DATA").unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);

        let mut inode = [0u8; 128];
        // File size = 4096 bytes, but let's test read_block_map
        inode[4..8].copy_from_slice(&4096u32.to_le_bytes());
        // Inode flags: 0 (block map)
        // Triple indirect is at i_block[14 * 4] = offset 40 + 14 * 4 = 96
        inode[96..100].copy_from_slice(&100u32.to_le_bytes());

        let blocks = resolver.read_block_map(&inode, 13 * 4096).unwrap();
        assert!(blocks.contains(&400));
    }

    #[test]
    fn test_ext2_32byte_bgdt_and_128byte_inode() {
        let meta = ExtMeta::new_ext2(32 * 1024 * 1024, Some("EXT2VOL")).unwrap();
        assert_eq!(meta.inode_size, 128);
        assert_eq!(meta.bgdt_entry_size, 32);
        assert!(!meta.features.has_extents);
        assert!(!meta.features.has_64bit);

        let layout = GroupLayout::compute(&meta, 0);
        assert_eq!(
            layout.inode_table_blocks,
            (meta.inodes_per_group * 128).div_ceil(meta.block_size)
        );
    }
}
