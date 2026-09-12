// SPDX-License-Identifier: MIT

//! Block Map (Legacy) addressing logic for Ext2/3.

use zerocopy::byteorder::little_endian::U32;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The 15-entry array stored in i_block for Ext2/3
/// 12 Direct + 1 Indirect + 1 Double + 1 Triple
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
#[derive(Default)]
pub struct BlockMapArray {
    pub direct: [U32; 12],
    pub indirect: U32,
    pub double_indirect: U32,
    pub triple_indirect: U32,
}

const _: () = {
    assert!(core::mem::size_of::<BlockMapArray>() == 60);
    assert!(core::mem::align_of::<BlockMapArray>() == 1);
    assert!(core::mem::offset_of!(BlockMapArray, indirect) == 48);
};

use crate::allocator::ExtAllocator;
use crate::core::{FsInjectorError, FsInjectorResult};
use crate::meta::ExtMeta;
use rimio::prelude::*;

/// Process a list of allocated blocks and map them into the Ext2/3 BlockMap structure,
/// allocating Indirect/Double/Triple blocks as needed.
pub fn build_block_map<IO: RimIO + ?Sized>(
    io: &mut IO,
    allocator: &mut ExtAllocator,
    meta: &ExtMeta,
    blocks: &[u32],
) -> FsInjectorResult<BlockMapArray> {
    let mut map = BlockMapArray::default();
    let ptrs_per_block = meta.block_size / 4;

    // Direct Blocks (0-11)
    let direct_count = blocks.len().min(12);
    for (slot, &block) in map.direct.iter_mut().zip(&blocks[..direct_count]) {
        *slot = block.into();
    }

    // Handle Indirect
    let mut remaining = if blocks.len() > 12 {
        &blocks[12..]
    } else {
        &[]
    };
    if remaining.is_empty() {
        return Ok(map);
    }

    // Indirect Block
    {
        let count = remaining.len().min(ptrs_per_block as usize);
        let chunk = &remaining[..count];

        let ind_blocks = allocator
            .blocks
            .allocate_blocks_list(io, 1)
            .map_err(|_| FsInjectorError::Other("Failed to allocate indirect block"))?;
        let indirect_block = ind_blocks.0[0].start as u32;
        map.indirect = indirect_block.into();

        write_pointers(io, meta, indirect_block, chunk)?;
        remaining = &remaining[count..];
    }

    if remaining.is_empty() {
        return Ok(map);
    }

    // Double Indirect Block
    {
        let blocks_per_indirect = ptrs_per_block as usize;
        let max_data_blocks = blocks_per_indirect * blocks_per_indirect;
        let count = remaining.len().min(max_data_blocks);
        let chunk = &remaining[..count];

        let dbl_blocks = allocator
            .blocks
            .allocate_blocks_list(io, 1)
            .map_err(|_| FsInjectorError::Other("Failed to allocate double indirect block"))?;
        let double_indirect_block = dbl_blocks.0[0].start as u32;
        map.double_indirect = double_indirect_block.into();

        let num_indirects = chunk.len().div_ceil(blocks_per_indirect) as u64;
        let i_handle = allocator
            .blocks
            .allocate_blocks_list(io, num_indirects)
            .map_err(|_| {
                FsInjectorError::Other("Failed to allocate indirect blocks for double indirect")
            })?;

        let i_blocks_vec = i_handle.to_units();
        write_pointers(io, meta, double_indirect_block, &i_blocks_vec)?;

        for (i, &indirect_block) in i_blocks_vec.iter().enumerate() {
            let start = i * blocks_per_indirect;
            let end = (start + blocks_per_indirect).min(chunk.len());
            let sub_chunk = &chunk[start..end];
            write_pointers(io, meta, indirect_block, sub_chunk)?;
        }

        remaining = &remaining[count..];
    }

    if !remaining.is_empty() {
        return Err(FsInjectorError::Other(
            "File too large for Double Indirect mapping (Triple Indirect not impl)",
        ));
    }

    Ok(map)
}

fn write_pointers<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    block: u32,
    pointers: &[u32],
) -> FsInjectorResult {
    let offset = block as u64 * meta.block_size as u64;
    let block_size = meta.block_size as usize;
    let ptrs_bytes_len = pointers.len() * 4;
    if ptrs_bytes_len > block_size {
        return Err(FsInjectorError::Other("Pointer count exceeds block size"));
    }

    let mut chunk_buf = [0u8; 256];
    let mut written = 0;
    while written < pointers.len() {
        let count = (pointers.len() - written).min(chunk_buf.len() / 4);
        for i in 0..count {
            chunk_buf[i * 4..i * 4 + 4].copy_from_slice(&pointers[written + i].to_le_bytes());
        }
        io.write_at(offset + (written as u64 * 4), &chunk_buf[..count * 4])?;
        written += count;
    }

    if ptrs_bytes_len < block_size {
        io.zero_at(
            offset + ptrs_bytes_len as u64,
            (block_size - ptrs_bytes_len) as u64,
        )?;
    }

    Ok(())
}
