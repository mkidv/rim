// SPDX-License-Identifier: MIT

//! ext4 block group descriptor table writing and initialization.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::allocator::{ExtAllocator, ExtBlockBitmap, ExtInodeBitmap};
use crate::core::bitmap::BitmapDriver;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::ExtMeta;
use crate::types::ExtBlockGroupDesc;
use crate::types::GroupLayout;

#[derive(Default)]
pub struct BlockGroupFeature {
    group_layouts: Vec<GroupLayout>,
    block_size: u32,
    blocks_per_group: u32,
    inodes_per_group: u32,
}

impl BlockGroupFeature {
    pub fn new() -> Self {
        Self::default()
    }

    fn init_block_bitmap<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &ExtMeta,
        group: usize,
        layout: &GroupLayout,
    ) -> FsFeatureResult<()> {
        let bm_meta = ExtBlockBitmap {
            meta,
            bitmap_block: layout.block_bitmap_block,
        };
        let mut view = BitmapDriver::new(&bm_meta);

        // Initialize with 1s (padding)
        view.format_with(io, 0xFF)?;

        let group_block_count = meta.group_total_blocks(group) as u64;

        view.set_bits_range(io, 0, group_block_count, false)?;

        let reserved_count = layout.first_data_block - layout.group_start;
        view.set_bits_range(io, 0, reserved_count, true)?;

        // Group 0: Mark Root (1st data block) and Lost+Found (2nd data block)
        // These are statically allocated by RootDirFeature and LostFoundFeature
        // but not tracked by the allocator during formatting.
        if group == 0 {
            // Bit indices for these blocks are reserved_count and reserved_count + 1
            view.set_bits_range(io, reserved_count, 2, true)?;
        }

        view.flush(io)?;
        Ok(())
    }

    fn init_inode_bitmap<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &ExtMeta,
        group: usize,
        layout: &GroupLayout,
    ) -> FsFeatureResult<()> {
        let bm_meta = ExtInodeBitmap {
            meta,
            bitmap_block: layout.inode_bitmap_block,
        };
        let mut view = BitmapDriver::new(&bm_meta);

        // Initialize with 1s (padding)
        view.format_with(io, 0xFF)?;

        let group_inode_count = meta.group_total_inodes(group) as u64;

        view.set_bits_range(io, 0, group_inode_count, false)?;

        // Group 0: Mark reserved inodes (1..10) as used
        if group == 0 {
            view.set_bits_range(io, 0, 11, true)?;
        }

        view.flush(io)?;
        Ok(())
    }
}

impl<'p, IO: RimIO + ?Sized> FsSystemFeature<ExtMeta, ExtAllocator<'p>, IO> for BlockGroupFeature {
    fn name(&self) -> &str {
        "Block Group Table & Bitmaps"
    }

    fn prepare(&mut self, meta: &ExtMeta) -> FsFeatureResult<()> {
        self.block_size = meta.block_size;
        self.blocks_per_group = meta.blocks_per_group;
        self.inodes_per_group = meta.inodes_per_group;

        self.group_layouts.clear();
        for group in 0..meta.group_count as usize {
            let layout = GroupLayout::compute(meta, group as u32);
            self.group_layouts.push(layout);
        }

        Ok(())
    }

    fn allocate(&mut self, io: &mut IO, allocator: &mut ExtAllocator) -> FsFeatureResult<()> {
        for (group, layout) in self.group_layouts.iter().enumerate() {
            self.init_block_bitmap(io, allocator.blocks.params, group, layout)?;
            self.init_inode_bitmap(io, allocator.blocks.params, group, layout)?;
        }

        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &ExtAllocator) -> FsFeatureResult<()> {
        let bgdt_entry_size = allocator.blocks.params.bgdt_entry_size;
        let mut bgdt_buf = vec![0u8; bgdt_entry_size * self.group_layouts.len()];

        for (group_idx, layout) in self.group_layouts.iter().enumerate() {
            let total_blocks = allocator.blocks.params.group_total_blocks(group_idx);
            let metadata_blocks = layout.metadata_blocks();
            let data_blocks_used = allocator.blocks.allocated_in_group(group_idx);
            let used_blocks = metadata_blocks + data_blocks_used;
            let free_blocks = (total_blocks as u32).saturating_sub(used_blocks) as u16;

            let total_inodes = allocator.blocks.params.group_total_inodes(group_idx);
            let used_inodes = allocator
                .meta
                .allocated_in_group(group_idx, allocator.blocks.params.inodes_per_group);
            let free_inodes = (total_inodes as u32).saturating_sub(used_inodes) as u16;

            let used_dirs = if group_idx == 0 { 2 } else { 0 };

            let bgd = ExtBlockGroupDesc::new(
                layout.block_bitmap_block,
                layout.inode_bitmap_block,
                layout.inode_table_block,
                free_blocks,
                free_inodes,
                used_dirs,
            );

            let group_offset = group_idx * bgdt_entry_size;
            bgdt_buf[group_offset..group_offset + bgdt_entry_size]
                .copy_from_slice(&bgd.as_bytes()[..bgdt_entry_size]);
        }

        let bgdt_offset =
            (allocator.blocks.params.first_data_block as u64 + 1) * self.block_size as u64;
        io.write_at(bgdt_offset, &bgdt_buf)?;

        for layout in &self.group_layouts {
            let is_backup = layout.reserved_blocks > 0 && layout.group_id != 0;
            if is_backup {
                let sb_copy_offset = layout
                    .group_start
                    .checked_mul(self.block_size as u64)
                    .ok_or(crate::core::errors::FsFeatureError::InvalidConfiguration(
                        "EXT backup BGDT offset overflow",
                    ))?;
                let bgdt_copy_offset = sb_copy_offset.checked_add(self.block_size as u64).ok_or(
                    crate::core::errors::FsFeatureError::InvalidConfiguration(
                        "EXT backup BGDT offset overflow",
                    ),
                )?;
                io.write_at(bgdt_copy_offset, &bgdt_buf)?;
            }
        }

        Ok(())
    }
}
