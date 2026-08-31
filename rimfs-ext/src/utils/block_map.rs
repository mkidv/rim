// SPDX-License-Identifier: MIT

//! Indirect Block Logic for Ext2/3.

use crate::core::allocator::FsAllocator;

use crate::core::{FsInjectorError, FsInjectorResult};
use crate::{allocator::ExtAllocator, meta::ExtMeta, types::BlockMapArray};
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
    let _ptrs_sq = ptrs_per_block * ptrs_per_block;

    // Direct Blocks (0-11)
    let direct_count = blocks.len().min(12);
    map.direct[..direct_count].copy_from_slice(&blocks[..direct_count]);

    // Handle Indirect
    let mut remaining = if blocks.len() > 12 {
        &blocks[12..]
    } else {
        &[]
    };
    if remaining.is_empty() {
        return Ok(map);
    }

    // 1. Indirect Block (holds `ptrs_per_block` entries)
    {
        let count = remaining.len().min(ptrs_per_block as usize);
        let chunk = &remaining[..count];

        // Allocate Indirect Block
        let handle = allocator
            .allocate(io, 1)
            .map_err(|_| FsInjectorError::Other("Failed to allocate indirect block"))?;
        let indirect_block = handle.blocks.0[0].start as u32;
        map.indirect = indirect_block;

        // Write Pointers
        write_pointers(io, meta, indirect_block, chunk)?;

        remaining = &remaining[count..];
    }

    if remaining.is_empty() {
        return Ok(map);
    }

    // 2. Double Indirect Block (holds `ptrs_per_block` indirect blocks)
    {
        // Each indirect block covers `ptrs_per_block` data blocks
        let blocks_per_indirect = ptrs_per_block as usize;
        let max_data_blocks = blocks_per_indirect * blocks_per_indirect;
        let count = remaining.len().min(max_data_blocks);
        let chunk = &remaining[..count]; // These are data blocks

        // Allocate Double Indirect Block
        let handle = allocator
            .allocate(io, 1)
            .map_err(|_| FsInjectorError::Other("Failed to allocate double indirect block"))?;
        let double_indirect_block = handle.blocks.0[0].start as u32;
        map.double_indirect = double_indirect_block;

        // We need to split `chunk` into sub-chunks of `ptrs_per_block`
        // Allocate N indirect blocks
        let num_indirects = chunk.len().div_ceil(blocks_per_indirect);
        let i_handle = allocator.allocate(io, num_indirects).map_err(|_| {
            FsInjectorError::Other("Failed to allocate indirect blocks for double indirect")
        })?;

        // Write the Double Indirect Block (it points to indirect blocks)
        let i_blocks_vec = i_handle.blocks.to_units();
        write_pointers(io, meta, double_indirect_block, &i_blocks_vec)?;

        // For each indirect block, write its data pointers
        for (i, &indirect_block) in i_blocks_vec.iter().enumerate() {
            let start = i * blocks_per_indirect;
            let end = (start + blocks_per_indirect).min(chunk.len());
            let sub_chunk = &chunk[start..end];
            write_pointers(io, meta, indirect_block, sub_chunk)?;
        }

        remaining = &remaining[count..];
    }

    if remaining.is_empty() {
        return Ok(map);
    }

    // 3. Triple Indirect Block
    // Simplified: supports very large files, logic similar to Double but one level deeper
    // For now, if we exceed Double Indirect (~4GB with 4k blocks), we error out or implement logic.
    // Double Indirect covers: 1024 * 1024 blocks = 1M blocks = 4GB.
    // So it's likely sufficient for this toy implementation.
    // If needed, we implement Triple.

    if !remaining.is_empty() {
        return Err(FsInjectorError::Other(
            "File too large for Double Indirect mapping (Tripel Indirect not impl)",
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
    let offset = allocator_offset(meta, block);
    // Convert u32 slice to bytes
    let mut buf = alloc::vec![0u8; meta.block_size as usize];

    // Safe manual copy to little endian
    for (i, &ptr) in pointers.iter().enumerate() {
        if i * 4 + 4 > buf.len() {
            break;
        }
        buf[i * 4..i * 4 + 4].copy_from_slice(&ptr.to_le_bytes());
    }

    io.write_block_best_effort(offset, &buf, meta.block_size as usize)?;
    Ok(())
}

fn allocator_offset(meta: &ExtMeta, block: u32) -> u64 {
    // Should reuse `ExtAllocator` logic or just standard formula
    // assuming `block` is absolute FS block number
    block as u64 * meta.block_size as u64
}
