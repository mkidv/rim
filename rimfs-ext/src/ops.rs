// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::ToString;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::core::allocator::FsAllocator;
use crate::core::{FsInjectorError, FsInjectorResult};
use crate::{
    allocator::ExtAllocator, group_layout::GroupLayout, meta::ExtMeta, types::ExtLostFound,
};
use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::core::utils::align::pad_to_size;

pub fn pad_directory_block(buf: &mut Vec<u8>, block_size: usize) {
    if buf.is_empty() {
        return;
    }

    // Ensure buffer handles at least block_size
    if buf.len() >= block_size {
        pad_to_size(buf, block_size);
        return;
    }

    // Find the last entry and adjust its rec_len
    let mut pos = 0usize;
    let mut last_entry_pos = 0usize;

    while pos + 8 <= buf.len() {
        let rec_len = u16::from_le_bytes([buf[pos + 4], buf[pos + 5]]) as usize;
        if rec_len == 0 || pos + rec_len > buf.len() {
            break;
        }
        last_entry_pos = pos;
        pos += rec_len;
    }

    // Adjust the last entry's rec_len to reach block_size
    let new_rec_len = (block_size - last_entry_pos) as u16;
    if last_entry_pos + 5 < buf.len() {
        buf[last_entry_pos + 4] = (new_rec_len & 0xFF) as u8;
        buf[last_entry_pos + 5] = ((new_rec_len >> 8) & 0xFF) as u8;
    }

    // Pad buffer to block size
    pad_to_size(buf, block_size);
}

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

pub fn create_lost_found<IO: RimIO + ?Sized>(
    io: &mut IO,
    allocator: &mut ExtAllocator,
    meta: &ExtMeta,
    parent_buf: &mut Vec<u8>,
    parent_child_dir_count: &mut u16,
    used_dirs_per_group: &mut [u16],
) -> FsInjectorResult {
    // 1. Allocate a block
    let handle = allocator
        .allocate(io, 1)
        .map_err(|_| FsInjectorError::Other("Allocation failed for lost+found"))?;
    let block = handle.blocks.0[0].start as u32;

    // 2. Write directory content
    let dir_buf = ExtLostFound::create_dir_block(meta.block_size as usize);
    let offset = allocator.blocks.block_offset(block as u64);
    io.write_block_best_effort(offset, &dir_buf, meta.block_size as usize)?;

    // 3. Write Inode (Inode 11)
    let inode = ExtLostFound::INODE;
    let inode_data = ExtLostFound::create_inode(meta.block_size, block);
    let inode_buf = inode_data.as_bytes(); // Using as_bytes() via IntoBytes derived on ExtInode
    write_inode(io, meta, inode, inode_buf)?;

    // 4. Add entry to parent (Root) buffer
    let entry = ExtLostFound::entry();
    entry.to_raw_buffer(parent_buf);

    // Increment parent link count (subdirectories increase parent nlink)
    *parent_child_dir_count += 1;

    // Mark directory as used in group stats
    let inode_index = inode - 1;
    let group = (inode_index / meta.inodes_per_group) as usize;
    if let Some(count) = used_dirs_per_group.get_mut(group) {
        *count += 1;
    }

    Ok(())
}
