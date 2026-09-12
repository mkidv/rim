// SPDX-License-Identifier: MIT
//! EXT Directory block and Inode writing utilities.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::core::utils::align::pad_to_size;
use crate::core::{FsInjectorError, FsInjectorResult};
use crate::meta::ExtMeta;
use crate::types::{ExtDirEntryHeader, GroupLayout};
use rimio::prelude::*;
use zerocopy::FromBytes;

/// Pads a directory block buffer to the requested block size,
pub fn pad_directory_block(buf: &mut Vec<u8>, block_size: usize) {
    if buf.is_empty() {
        buf.resize(block_size, 0);
        if let Ok((header, _)) = ExtDirEntryHeader::mut_from_prefix(buf.as_mut_slice()) {
            header.rec_len = (block_size as u16).into();
        }
        return;
    }

    if buf.len() >= block_size {
        pad_to_size(buf, block_size);
        return;
    }

    let mut pos = 0usize;
    let mut last_entry_pos = 0usize;

    while pos + 8 <= buf.len() {
        let Ok((header, _)) = ExtDirEntryHeader::ref_from_prefix(&buf[pos..]) else {
            break;
        };
        let rec_len = header.rec_len.get() as usize;
        if rec_len == 0 || pos + rec_len > buf.len() {
            break;
        }
        last_entry_pos = pos;
        pos += rec_len;
    }

    let new_rec_len = (block_size - last_entry_pos) as u16;
    if let Ok((header, _)) = ExtDirEntryHeader::mut_from_prefix(&mut buf[last_entry_pos..]) {
        header.rec_len = new_rec_len.into();
    }

    pad_to_size(buf, block_size);
}

/// Writes raw inode bytes into the inode table on disk.
pub fn write_inode<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    inode: u32,
    data: &[u8],
) -> FsInjectorResult {
    // inode numbers are 1-based.
    if inode < 1 {
        return Err(FsInjectorError::Other("Invalid Inode 0"));
    }
    let inode_index = inode - 1;
    let inodes_per_group = meta.inodes_per_group;
    let group = inode_index / inodes_per_group;
    let index_in_group = inode_index % inodes_per_group;

    let layout = GroupLayout::compute(meta, group);
    let table_block = layout.inode_table_block;

    let inode_size = meta.inode_size as u64;
    let offset = (table_block * meta.block_size as u64) + (index_in_group as u64 * inode_size);

    let write_len = (meta.inode_size as usize).min(data.len());
    io.write_at(offset, &data[..write_len])?;
    Ok(())
}
