// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::ToString;

use crate::core::FsInjectorResult;
use crate::{allocator::ExtAllocator, constant::*, group_layout::GroupLayout, meta::ExtMeta};
use rimio::prelude::*;
use zerocopy::IntoBytes;

pub fn flush_superblock<IO: RimIO + ?Sized>(
    io: &mut IO,
    allocator: &ExtAllocator,
    meta: &ExtMeta,
) -> FsInjectorResult {
    let mut total_used_blocks: u64 = 0;
    let mut total_used_inodes: u64 = 0;

    let count = meta.group_count();

    for g in 0..count {
        let layout = GroupLayout::compute(meta, g as u32);
        let metadata_overhead = layout.metadata_blocks();
        let data_used = allocator.blocks.allocated_in_group(g);
        total_used_blocks += (metadata_overhead as u64) + (data_used as u64);

        total_used_inodes += allocator.meta.allocated_in_group(g, meta.inodes_per_group) as u64;
    }

    let free_blocks = meta.block_count.saturating_sub(total_used_blocks);
    let free_inodes = meta.inode_count.saturating_sub(total_used_inodes);

    // Primary Superblock
    io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x0C, free_blocks as u32)?; // s_free_blocks_count_lo
    io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x10, free_inodes as u32)?; // s_free_inodes_count

    // Backup Superblocks
    for group_id in 1..count {
        let has_super =
            !meta.features.has_sparse_super || crate::utils::is_sparse_super_group(group_id as u32);
        if has_super {
            let group_start_block =
                meta.first_data_block as u64 + group_id as u64 * meta.blocks_per_group as u64;
            let sb_copy_offset = group_start_block * meta.block_size as u64;
            io.write_u32_at(sb_copy_offset + 0x0C, free_blocks as u32)?;
            io.write_u32_at(sb_copy_offset + 0x10, free_inodes as u32)?;
        }
    }

    Ok(())
}

pub fn flush_bgdt<IO: RimIO + ?Sized>(
    io: &mut IO,
    allocator: &ExtAllocator,
    meta: &ExtMeta,
    used_dirs_per_group: &[u16],
) -> FsInjectorResult {
    let count = meta.group_count();
    let bgdt_start_offset = bgdt_offset(meta);

    let bgdt_entry_size = meta.bgdt_entry_size;
    let mut bgdt_buf = vec![0u8; bgdt_entry_size * count];

    for group_index in 0..count {
        let layout = GroupLayout::compute(meta, group_index as u32);
        let metadata_overhead = layout.metadata_blocks();
        let data_used = allocator.blocks.allocated_in_group(group_index);
        let used_blocks = metadata_overhead + data_used;

        let used_inodes = allocator
            .meta
            .allocated_in_group(group_index, meta.inodes_per_group);

        let free_blocks = (meta.group_total_blocks(group_index) as u32).saturating_sub(used_blocks);
        let free_inodes = (meta.group_total_inodes(group_index) as u32).saturating_sub(used_inodes);

        let default_dirs = if group_index == 0 { 2 } else { 0 };
        let used_dirs = used_dirs_per_group
            .get(group_index)
            .copied()
            .unwrap_or(default_dirs);

        let bgd = crate::types::ExtBlockGroupDesc::new(
            layout.block_bitmap_block,
            layout.inode_bitmap_block,
            layout.inode_table_block,
            free_blocks as u16,
            free_inodes as u16,
            used_dirs,
        );

        let entry_offset = group_index * bgdt_entry_size;
        bgdt_buf[entry_offset..entry_offset + bgdt_entry_size]
            .copy_from_slice(&bgd.as_bytes()[..bgdt_entry_size]);
    }

    // Write Primary BGDT
    io.write_at(bgdt_start_offset, &bgdt_buf)?;

    // Write Backup BGDTs
    for group_id in 1..count {
        let layout = GroupLayout::compute(meta, group_id as u32);
        if layout.reserved_blocks > 0 {
            let sb_copy_offset = layout.group_start * (meta.block_size as u64);
            let bgdt_copy_offset = sb_copy_offset.checked_add(meta.block_size as u64).ok_or(
                crate::core::FsInjectorError::Invalid("EXT backup BGDT offset overflow"),
            )?;
            io.write_at(bgdt_copy_offset, &bgdt_buf)?;
        }
    }

    Ok(())
}

fn bgdt_offset(meta: &ExtMeta) -> u64 {
    let first_data_block = meta.first_data_block as u64;
    (first_data_block + 1) * meta.block_size as u64
}
