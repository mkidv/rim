// SPDX-License-Identifier: MIT

//! ext2/3/4 directory tree and extent resolver.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::constant::*;
use crate::core::resolver::*;
use crate::core::traits::FsMeta;
use crate::meta::ExtMeta;
#[cfg(test)]
use crate::types::GroupLayout;
use crate::types::{
    BlockMapArray, ExtDirEntryHeader, ExtExtent, ExtExtentHeader, ExtExtentIndex, ExtInodeHeader,
};
use rimio::prelude::*;
use zerocopy::FromBytes;

pub struct ExtResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExtMeta,
    inode_cache: [(u32, Vec<u8>); 4],
    cache_head: usize,
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExtMeta) -> Self {
        Self {
            io,
            meta,
            inode_cache: Default::default(),
            cache_head: 0,
        }
    }
}

use crate::core::resolver::walker::WalkerDataSource;

impl<'a, IO: RimRead + ?Sized> WalkerDataSource for ExtResolver<'a, IO> {
    type Entry = ExtDirEntry;
    type NodeId = u32;

    fn root_node(&self) -> Self::NodeId {
        EXT_ROOT_INODE
    }

    fn find_entry(
        &mut self,
        dir_inode: Self::NodeId,
        name: &str,
    ) -> FsResolverResult<Option<Self::Entry>> {
        self.find_in_dir(dir_inode, name)
    }

    fn is_dir(&self, entry: &Self::Entry) -> bool {
        entry.is_dir()
    }

    fn entry_node(&self, entry: &Self::Entry) -> Self::NodeId {
        entry.inode
    }
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    fn get_inode_table_block(&mut self, group: u32) -> FsResolverResult<u64> {
        let entry = crate::utils::read_group_descriptor(self.io, self.meta, group)?;
        let lo = entry.bg_inode_table_lo.get() as u64;
        let hi = if self.meta.features.has_64bit {
            entry.bg_inode_table_hi.get() as u64
        } else {
            0
        };
        let block = lo | (hi << 32);
        if block == 0 || block >= self.meta.block_count {
            return Err(FsResolverError::Invalid("Invalid inode table block"));
        }
        Ok(block)
    }

    /// Read inode raw bytes from inode table (with MRU cache)
    pub(crate) fn read_inode(&mut self, inode_num: u32) -> FsResolverResult<Vec<u8>> {
        if inode_num == 0 || inode_num as u64 > self.meta.inode_count {
            return Err(FsResolverError::Invalid("Invalid inode number"));
        }

        for (cached_num, buf) in &self.inode_cache {
            if *cached_num == inode_num {
                return Ok(buf.clone());
            }
        }

        let inode_index = inode_num - 1;
        let group = inode_index / self.meta.inodes_per_group;
        let index_in_group = inode_index % self.meta.inodes_per_group;

        let inode_table_block = self.get_inode_table_block(group)?;

        let inode_size = self.meta.inode_size as u64;
        let offset = (inode_table_block * self.meta.block_size as u64)
            + (index_in_group as u64 * inode_size);

        let mut buf = vec![0u8; inode_size as usize];
        self.io
            .read_at(offset, &mut buf)
            .map_err(FsResolverError::IO)?;

        self.inode_cache[self.cache_head] = (inode_num, buf.clone());
        self.cache_head = (self.cache_head + 1) % self.inode_cache.len();

        Ok(buf)
    }

    /// Check if inode is a directory
    fn inode_is_dir(&self, inode_buf: &[u8]) -> bool {
        let mode = ExtInodeHeader::ref_from_prefix(inode_buf)
            .map(|(h, _)| h.i_mode.get())
            .unwrap_or(0);
        (mode & 0xF000) == 0x4000
    }

    /// Get size of file from inode
    fn inode_size(&self, inode_buf: &[u8]) -> u64 {
        ExtInodeHeader::ref_from_prefix(inode_buf)
            .map(|(h, _)| (h.i_size_high.get() as u64) << 32 | h.i_size_lo.get() as u64)
            .unwrap_or(0)
    }

    /// Recursively collect leaf extents from an extent node
    fn collect_extents_from_node(
        &mut self,
        header: &ExtExtentHeader,
        entries_buf: &[u8],
        max_depth: u16,
        out: &mut Vec<ExtExtent>,
    ) -> FsResolverResult<()> {
        if header.eh_magic.get() != EXT_EXTENT_HEADER_MAGIC {
            return Err(FsResolverError::Invalid("Invalid extent header magic"));
        }

        let entries_count = header.eh_entries.get() as usize;

        if header.eh_depth.get() == 0 {
            let bytes = entries_buf
                .get(..entries_count * core::mem::size_of::<ExtExtent>())
                .ok_or(FsResolverError::Invalid("Truncated extent array"))?;
            let entries = <[ExtExtent]>::ref_from_bytes(bytes)
                .map_err(|_| FsResolverError::Invalid("Invalid extent array"))?;
            out.extend_from_slice(entries);
        } else {
            if max_depth == 0 {
                return Err(FsResolverError::Invalid("Extent tree depth exceeded limit"));
            }
            let bytes = entries_buf
                .get(..entries_count * core::mem::size_of::<ExtExtentIndex>())
                .ok_or(FsResolverError::Invalid("Truncated extent index array"))?;
            let entries = <[ExtExtentIndex]>::ref_from_bytes(bytes)
                .map_err(|_| FsResolverError::Invalid("Invalid extent index array"))?;
            let mut child_buf = vec![0u8; self.meta.block_size as usize];
            for entry in entries {
                let child_offset = entry.leaf_physical_block() * self.meta.block_size as u64;
                self.io.read_at(child_offset, &mut child_buf)?;
                let (child_header, tail) = ExtExtentHeader::ref_from_prefix(&child_buf)
                    .map_err(|_| FsResolverError::Invalid("Failed to read child extent header"))?;
                self.collect_extents_from_node(child_header, tail, max_depth - 1, out)?;
            }
        }

        Ok(())
    }

    /// Read extents from inode buffer
    pub(crate) fn read_extents(&mut self, inode_buf: &[u8]) -> FsResolverResult<Vec<ExtExtent>> {
        let i_flags = ExtInodeHeader::ref_from_prefix(inode_buf)
            .ok()
            .map(|(h, _)| h.i_flags.get())
            .unwrap_or(0);
        if i_flags & EXT_INODE_FLAG_EXTENTS == 0 {
            return Err(FsResolverError::Invalid(
                "Inode does not use extents (block map not supported)",
            ));
        }

        // Extent header is at offset 40 in inode
        let header = ExtExtentHeader::ref_from_bytes(
            inode_buf
                .get(40..52)
                .ok_or(FsResolverError::Invalid("Truncated inode extent header"))?,
        )
        .ok()
        .ok_or(FsResolverError::Invalid("Failed to read extent header"))?;

        if header.eh_magic.get() != EXT_EXTENT_HEADER_MAGIC {
            return Err(FsResolverError::Invalid("Invalid extent header magic"));
        }

        let mut extents = Vec::new();
        // Inode extent buffer is at offset 52..100 (48 bytes max in 128/256-byte inode header)
        let entries_slice = inode_buf
            .get(52..100)
            .ok_or(FsResolverError::Invalid("Truncated inode extent table"))?;
        self.collect_extents_from_node(header, entries_slice, 5, &mut extents)?;

        Ok(extents)
    }

    /// Read blocks from Block Map (Ext2/3)
    fn read_block_map(&mut self, inode_buf: &[u8], size: usize) -> FsResolverResult<Vec<u32>> {
        let map = BlockMapArray::ref_from_bytes(
            inode_buf
                .get(40..100)
                .ok_or(FsResolverError::Invalid("Truncated inode block map"))?,
        )
        .map_err(|_| FsResolverError::Invalid("Failed to read block map"))?;

        let block_size = self.meta.block_size as usize;
        let ptrs_per_block = block_size / 4;
        let dbl_ptrs = ptrs_per_block * ptrs_per_block;
        let trpl_ptrs = dbl_ptrs * ptrs_per_block;

        let blocks_count = size.div_ceil(block_size);
        let mut blocks = Vec::with_capacity(blocks_count);

        // 1. Direct Blocks (0..12)
        for &blk in &map.direct {
            if blocks.len() >= blocks_count {
                break;
            }
            blocks.push(blk.get());
        }

        if blocks.len() >= blocks_count {
            return Ok(blocks);
        }

        // 2. Single Indirect Block
        let ind_limit = blocks_count.min(12 + ptrs_per_block);
        if map.indirect.get() != 0 {
            self.read_indirect_block(map.indirect.get(), &mut blocks, ind_limit)?;
        }
        while blocks.len() < ind_limit {
            blocks.push(0);
        }

        if blocks.len() >= blocks_count {
            return Ok(blocks);
        }

        // 3. Double Indirect Block
        let dbl_limit = blocks_count.min(12 + ptrs_per_block + dbl_ptrs);
        if map.double_indirect.get() != 0 {
            let mut indirects = Vec::new();
            self.read_indirect_block(map.double_indirect.get(), &mut indirects, ptrs_per_block)?;
            while indirects.len() < ptrs_per_block {
                indirects.push(0);
            }

            for &indirect_blk in &indirects {
                if blocks.len() >= dbl_limit {
                    break;
                }
                let cur_limit = dbl_limit.min(blocks.len() + ptrs_per_block);
                if indirect_blk != 0 {
                    self.read_indirect_block(indirect_blk, &mut blocks, cur_limit)?;
                }
                while blocks.len() < cur_limit {
                    blocks.push(0);
                }
            }
        }
        while blocks.len() < dbl_limit {
            blocks.push(0);
        }

        if blocks.len() >= blocks_count {
            return Ok(blocks);
        }

        // 4. Triple Indirect Block
        let trpl_limit = blocks_count.min(12 + ptrs_per_block + dbl_ptrs + trpl_ptrs);
        if map.triple_indirect.get() != 0 {
            let mut double_indirects = Vec::new();
            self.read_indirect_block(
                map.triple_indirect.get(),
                &mut double_indirects,
                ptrs_per_block,
            )?;
            while double_indirects.len() < ptrs_per_block {
                double_indirects.push(0);
            }

            for &double_blk in &double_indirects {
                if blocks.len() >= trpl_limit {
                    break;
                }
                let cur_dbl_limit = trpl_limit.min(blocks.len() + dbl_ptrs);
                if double_blk != 0 {
                    let mut indirects = Vec::new();
                    self.read_indirect_block(double_blk, &mut indirects, ptrs_per_block)?;
                    while indirects.len() < ptrs_per_block {
                        indirects.push(0);
                    }
                    for &indirect_blk in &indirects {
                        if blocks.len() >= cur_dbl_limit {
                            break;
                        }
                        let cur_limit = cur_dbl_limit.min(blocks.len() + ptrs_per_block);
                        if indirect_blk != 0 {
                            self.read_indirect_block(indirect_blk, &mut blocks, cur_limit)?;
                        }
                        while blocks.len() < cur_limit {
                            blocks.push(0);
                        }
                    }
                }
                while blocks.len() < cur_dbl_limit {
                    blocks.push(0);
                }
            }
        }
        while blocks.len() < trpl_limit {
            blocks.push(0);
        }

        Ok(blocks)
    }

    fn read_indirect_block(
        &mut self,
        block: u32,
        blocks: &mut Vec<u32>,
        limit: usize,
    ) -> FsResolverResult<()> {
        let ptrs_per_block = self.meta.block_size as usize / 4;
        let count_to_add = (limit.saturating_sub(blocks.len())).min(ptrs_per_block);
        if block == 0 {
            blocks.resize(blocks.len() + count_to_add, 0);
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

        let i_flags = ExtInodeHeader::ref_from_prefix(&inode_buf)
            .ok()
            .map(|(h, _)| h.i_flags.get())
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
        let i_flags = ExtInodeHeader::ref_from_prefix(&inode_buf)
            .ok()
            .map(|(h, _)| h.i_flags.get())
            .unwrap_or(0);

        if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
            let extents = self.read_extents(&inode_buf)?;
            for extent in &extents {
                let phys_block = extent.physical_start();
                let len_blocks = extent.len() as usize;
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
                if blk != 0 {
                    offsets.push(blk as u64 * block_size as u64);
                }
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
                let Some((header, _)) = ExtDirEntryHeader::from_record(&buf[pos..]) else {
                    break;
                };
                let entry_inode = header.inode.get();
                let rec_len = header.rec_len.get() as usize;
                let name_len = header.name_len as usize;
                let file_type = header.file_type;

                if rec_len == 0 || rec_len > buf.len() - pos {
                    break; // End of directory or corrupt entry
                }

                if entry_inode != 0 && name_len > 0 && pos + 8 + name_len <= buf.len() {
                    let name_bytes = &buf[pos + 8..pos + 8 + name_len];
                    if let Ok(name) = core::str::from_utf8(name_bytes)
                        && name != "."
                        && name != ".."
                    {
                        entries.push(ExtDirEntry {
                            inode: entry_inode,
                            name: name.to_string(),
                            file_type,
                        });
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

    /// Find an entry by name in a directory with early exit and zero unnecessary allocations
    fn find_in_dir(&mut self, dir_inode: u32, name: &str) -> FsResolverResult<Option<ExtDirEntry>> {
        let inode_buf = self.read_inode(dir_inode)?;

        if !self.inode_is_dir(&inode_buf) {
            return Err(FsResolverError::Invalid("Not a directory"));
        }

        let dir_size = self.inode_size(&inode_buf) as usize;
        let block_size = self.meta.block_size as usize;
        let blocks_needed = dir_size.div_ceil(block_size);

        let i_flags = ExtInodeHeader::ref_from_prefix(&inode_buf)
            .ok()
            .map(|(h, _)| h.i_flags.get())
            .unwrap_or(0);

        let mut offsets = Vec::new();
        if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
            let extents = self.read_extents(&inode_buf)?;
            for extent in &extents {
                let phys_block = extent.physical_start();
                let len_blocks = extent.len() as usize;
                for blk_idx in 0..len_blocks {
                    if offsets.len() >= blocks_needed {
                        break;
                    }
                    let blk = phys_block + blk_idx as u64;
                    offsets.push(blk * block_size as u64);
                }
            }
        } else {
            let blocks = self.read_block_map(&inode_buf, dir_size)?;
            for blk in blocks {
                if blk != 0 {
                    offsets.push(blk as u64 * block_size as u64);
                }
            }
        }

        let target_bytes = name.as_bytes();
        let mut buf = vec![0u8; block_size];
        let mut total_read = 0usize;

        for offset in offsets {
            if total_read >= dir_size {
                break;
            }
            self.io
                .read_at(offset, &mut buf)
                .map_err(FsResolverError::IO)?;

            let mut pos = 0usize;
            while pos + 8 <= buf.len() && total_read + pos < dir_size {
                let Some((header, _)) = ExtDirEntryHeader::from_record(&buf[pos..]) else {
                    break;
                };
                let entry_inode = header.inode.get();
                let rec_len = header.rec_len.get() as usize;
                let name_len = header.name_len as usize;
                let file_type = header.file_type;

                if rec_len == 0 || rec_len > buf.len() - pos {
                    break;
                }

                if entry_inode != 0
                    && name_len == target_bytes.len()
                    && pos + 8 + name_len <= buf.len()
                {
                    let entry_name_bytes = &buf[pos + 8..pos + 8 + name_len];
                    if entry_name_bytes == target_bytes {
                        let name_str = core::str::from_utf8(entry_name_bytes).map_err(|_| {
                            FsResolverError::Invalid("Invalid UTF-8 in directory entry")
                        })?;
                        return Ok(Some(ExtDirEntry {
                            inode: entry_inode,
                            name: name_str.to_string(),
                            file_type,
                        }));
                    }
                }

                pos += rec_len;
            }
            total_read += block_size;
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

        let i_flags = ExtInodeHeader::ref_from_prefix(inode_buf)
            .ok()
            .map(|(h, _)| h.i_flags.get())
            .unwrap_or(0);

        if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
            let extents = self.read_extents(inode_buf)?;
            for ext in extents {
                let logical_offset = ext.ee_block.get() as u64 * block_size;
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

    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        let entry = crate::core::resolver::walker::walk_path(self, path)?
            .ok_or(FsResolverError::NotFound)?;

        let inode_buf = self.read_inode(entry.inode)?;
        let i_mode = ExtInodeHeader::ref_from_prefix(&inode_buf)
            .ok()
            .map(|(h, _)| h.i_mode.get())
            .unwrap_or(0);

        crate::ensure!(
            (i_mode & 0xF000) == 0xA000,
            FsResolverError::Invalid("Not a symlink")
        );

        let size = self.inode_size(&inode_buf) as usize;
        let i_blocks = ExtInodeHeader::ref_from_prefix(&inode_buf)
            .ok()
            .map(|(h, _)| h.i_blocks_lo.get())
            .unwrap_or(0);

        if i_blocks == 0 && size < 60 {
            // Fast symlink
            let target_bytes = inode_buf
                .get(40..40 + size)
                .ok_or(FsResolverError::Invalid(
                    "Inode buffer too small for fast symlink",
                ))?;
            String::from_utf8(target_bytes.to_vec())
                .map_err(|_| FsResolverError::Invalid("Invalid UTF-8 in symlink target"))
        } else {
            // Slow symlink
            let content = self.read_file_content(entry.inode)?;
            let target_slice = if content.len() >= size {
                &content[..size]
            } else {
                &content[..]
            };
            String::from_utf8(target_slice.to_vec())
                .map_err(|_| FsResolverError::Invalid("Invalid UTF-8 in symlink target"))
        }
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        match crate::core::resolver::walker::walk_path(self, path)? {
            Some(entry) => {
                let inode_buf = self.read_inode(entry.inode)?;
                Ok(self.parse_attributes(&inode_buf, entry.is_dir()))
            }
            None => Ok(FileAttributes::new_dir()),
        }
    }
}

impl<'a, IO: RimRead + ?Sized> ExtResolver<'a, IO> {
    /// Parse file attributes from inode buffer
    fn parse_attributes(&self, inode_buf: &[u8], is_dir: bool) -> FileAttributes {
        let inode = ExtInodeHeader::ref_from_prefix(inode_buf)
            .ok()
            .map(|(h, _)| h);
        let i_mode = inode.map(|h| h.i_mode.get()).unwrap_or(0);

        let i_uid_lo = inode.map(|h| h.i_uid.get()).unwrap_or(0) as u32;

        let i_gid_lo = inode.map(|h| h.i_gid.get()).unwrap_or(0) as u32;

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

        let i_atime = inode.map(|h| h.i_atime.get()).unwrap_or(0);

        let i_ctime = inode.map(|h| h.i_ctime.get()).unwrap_or(0);

        let i_mtime = inode.map(|h| h.i_mtime.get()).unwrap_or(0);

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
            eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
            eh_entries: 2.into(),
            eh_max: 4.into(),
            eh_depth: 0.into(),
            eh_generation: 0.into(),
        };
        inode[40..52].copy_from_slice(header.as_bytes());

        let ext1 = ExtExtent::new_48(0, 100, 5);
        let ext2 = ExtExtent::new_48(5, 200, 10);
        inode[52..64].copy_from_slice(ext1.as_bytes());
        inode[64..76].copy_from_slice(ext2.as_bytes());

        let extents = resolver.read_extents(&inode).unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].physical_start(), 100);
        assert_eq!({ extents[0].ee_len.get() }, 5);
        assert_eq!(extents[1].physical_start(), 200);
        assert_eq!({ extents[1].ee_len.get() }, 10);
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
            eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
            eh_entries: 2.into(),
            eh_max: 340.into(),
            eh_depth: 0.into(),
            eh_generation: 0.into(),
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
            eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
            eh_entries: 1.into(),
            eh_max: 4.into(),
            eh_depth: 1.into(),
            eh_generation: 0.into(),
        };
        inode[40..52].copy_from_slice(root_header.as_bytes());

        let index_entry = ExtExtentIndex::new_48(0, child_block_num);
        inode[52..64].copy_from_slice(index_entry.as_bytes());

        let extents = resolver.read_extents(&inode).unwrap();
        assert_eq!(extents.len(), 2);
        assert_eq!(extents[0].physical_start(), 1000);
        assert_eq!({ extents[0].ee_len.get() }, 20);
        assert_eq!(extents[1].physical_start(), 2000);
        assert_eq!({ extents[1].ee_len.get() }, 30);
    }

    #[test]
    fn test_48bit_physical_extent() {
        let large_phys = 0x1_0000_2000u64;
        let ext = ExtExtent::new_48(0, large_phys, 8);
        assert_eq!(ext.physical_start(), large_phys);
        assert_eq!({ ext.ee_start_hi.get() }, 1);
        assert_eq!({ ext.ee_start_lo.get() }, 0x2000);
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
            eh_magic: EXT_EXTENT_HEADER_MAGIC.into(),
            eh_entries: 1.into(),
            eh_max: 4.into(),
            eh_depth: 0.into(),
            eh_generation: 0.into(),
        };
        inode[40..52].copy_from_slice(header.as_bytes());

        // Uninitialized extent of 2 blocks (ee_len = 2 | 0x8000 = 0x8002 = 32770)
        let uninit_ext = ExtExtent::new_48(0, 50, 0x8002);
        assert!(uninit_ext.is_uninit());
        assert_eq!(uninit_ext.len(), 2);
        inode[52..64].copy_from_slice(uninit_ext.as_bytes());

        let layout = GroupLayout::compute(&meta, 0);
        let mut descriptor = [0u8; 64];
        descriptor[8..12].copy_from_slice(&(layout.inode_table_block as u32).to_le_bytes());
        io.write_at(
            (meta.first_data_block as u64 + 1) * meta.block_size as u64,
            &descriptor[..meta.bgdt_entry_size],
        )
        .unwrap();
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

        let ptrs_per_block = 4096 / 4;
        let triple_indirect_start = 12 + ptrs_per_block + ptrs_per_block * ptrs_per_block;
        let blocks = resolver
            .read_block_map(&inode, (triple_indirect_start + 1) * 4096)
            .unwrap();
        assert_eq!(blocks[triple_indirect_start], 400);
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
