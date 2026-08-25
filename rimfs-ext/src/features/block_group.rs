// SPDX-License-Identifier: MIT

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use alloc::borrow::Cow;

use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::allocator::{ExtAllocator, ExtBlockBitmap, ExtInodeBitmap};
use crate::constant::*;
use crate::core::bitmap::BitmapDriver;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::group_layout::GroupLayout;
use crate::meta::ExtMeta;
use crate::types::ExtBlockGroupDesc;

#[derive(Default)]
pub struct BlockGroupFeature {
    bgdt_data: Option<Vec<u8>>,
    // Removed large cached bitmaps
    // We keep layouts to avoiding recomputing them in write phase,
    // though recomputing is cheap (math only).
    // Let's keep them for consistency with previous code structure.
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

        // Clear valid bits first (0..count) - mark as free
        view.set_bits_range(io, 0, group_block_count, false)?;

        // Mark reserved blocks (Superblock, GDT, Bitmaps, Inode Table)
        let reserved_count = layout.first_data_block - layout.group_start;
        view.set_bits_range(io, 0, reserved_count as u64, true)?;

        // Group 0: Mark Root (1st data block) and Lost+Found (2nd data block)
        // These are statically allocated by RootDirFeature and LostFoundFeature
        // but not tracked by the allocator during formatting.
        if group == 0 {
            // Bit indices for these blocks are reserved_count and reserved_count + 1
            view.set_bits_range(io, reserved_count as u64, 2, true)?;
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

        // Clear valid inode bits - mark as free
        view.set_bits_range(io, 0, group_inode_count, false)?;

        // Group 0: Mark reserved inodes (1..10) as used
        // Inode bitmap is 0-indexed (inode 1 is bit 0).
        // Standard reserved inodes are 1..10.
        // And usually we might want to mark 11 (lost+found) if we pre-allocate it?
        // But the Allocator handles 11+ allocation.
        // Wait, `ExtMetadataAllocator` starts at 12.
        // So we MUST mark 0..10 (inodes 1..11) as used?
        // Or 0..9 (inodes 1..10)?
        // Root is 2.
        // If we mark them used here, `allocate` calls for Root will find them used?
        // The `Allocator` logic finds free bits.
        // `ExtMetadataAllocator` handles dynamic allocation.
        // If we pre-allocate Root/LostFound in `write`, we should mark them used here OR there.
        // Since `fs/ext/features/root.rs` manually writes the inode but does NOT allocate it via allocator,
        // we should mark it used here to prevent double allocation.

        if group == 0 {
            // Mark 1..11 (bits 0..10) as used.
            // 1..10 = Reserved.
            // 11 = Next free?
            // `ExtMetadataAllocator` starts at 12.
            // So we mark 0..11 (12 bits) as used (Inodes 1..12).
            // Wait, Inode 11 is Lost+Found.
            // Inode 2 is Root.

            // Mark bits 0 to 10 (Inodes 1 to 11).
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

        let bgdt_buf = vec![0u8; meta.bgdt_entry_size * meta.group_count as usize];
        self.group_layouts.clear();

        for group in 0..meta.group_count as usize {
            let layout = GroupLayout::compute(meta, group as u32);
            self.group_layouts.push(layout);
        }

        self.bgdt_data = Some(bgdt_buf);

        Ok(())
    }

    fn allocate(&mut self, io: &mut IO, allocator: &mut ExtAllocator) -> FsFeatureResult<()> {
        // Initialize bitmaps on disk for ALL groups.
        // This ensures the disk state is valid for subsequent persistent allocations.

        for (group, layout) in self.group_layouts.iter().enumerate() {
            self.init_block_bitmap(io, allocator.blocks.params, group, layout)?;
            self.init_inode_bitmap(io, allocator.blocks.params, group, layout)?;
        }

        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &ExtAllocator) -> FsFeatureResult<()> {
        let mut bgdt_buf = self
            .bgdt_data
            .as_ref()
            .cloned()
            .ok_or(crate::core::errors::FsFeatureError::NotPrepared)?;

        let bgdt_entry_size = allocator.blocks.params.bgdt_entry_size;

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

            // --- Write Features ---

            // We do NOT write bitmaps here. They are already initialized by `allocate`
            // and potentially modified by the allocator (Root/Lost+Found).
            // Writing them again here (especially regenerating them) would overwrite
            // allocations made during formatting!

            // Sparse BGDT copies logic
            let is_backup = layout.reserved_blocks > 0 && layout.group_id != 0;
            if is_backup {
                let sb_copy_offset = (layout.group_start * self.block_size) as u64;
                let bgdt_copy_offset = sb_copy_offset + self.block_size as u64;
                io.write_at(bgdt_copy_offset, &bgdt_buf)?;
            }
        }

        // Write Primary BGDT
        let bgdt_offset = (EXT_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * self.block_size as u64;
        io.write_at(bgdt_offset, &bgdt_buf)?;

        Ok(())
    }
}
