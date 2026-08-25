// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::boxed::Box;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::ExtAllocator;
use crate::core::feature::FsSystemFeature;
use crate::core::{FsFormatterResult, formatter::FsFormatter};
use crate::features::{
    block_group::BlockGroupFeature, inode_table::InodeTableFeature, lost_found::LostFoundFeature,
    root::RootDirFeature, superblock::ExtSuperblockFeature,
};
use crate::meta::ExtMeta;

pub struct ExtFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExtMeta,
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for ExtFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult {
        if full_format {
            crate::core::formatter::zero_cluster_heap(self.io, self.meta)?;
        }

        // Define the sequence of features to apply
        // Note: Using Box to erase types and iterate, but we need strict ordering anyway.
        // We can just execute them sequentially without a Vec<Box> if we want to avoid allocation,
        // but Vec<Box> is cleaner for "Orchestrator" pattern.

        // Since FsSystemFeature has generic IO, we need to specify it.
        // And Allocator A is ExtAllocator.
        let mut features: Vec<Box<dyn FsSystemFeature<ExtMeta, ExtAllocator<'a>, IO>>> = vec![
            Box::new(ExtSuperblockFeature::new()),
            Box::new(BlockGroupFeature::new()),
            Box::new(InodeTableFeature::new()),
            // Directories must come after InodeTable if they write to it (they do)
            Box::new(RootDirFeature::new()),
            Box::new(LostFoundFeature::new()),
        ];

        let mut allocator = ExtAllocator::new(self.meta);

        for feature in &mut features {
            // 1. Prepare
            feature.prepare(self.meta)?;

            // 2. Allocate
            feature.allocate(self.io, &mut allocator)?;
        }

        for feature in &mut features {
            // 3. Write
            feature.write(self.io, &allocator)?;
        }

        // Flush final superblock and BGDT with exact counts
        crate::updates::flush_superblock(self.io, &allocator, self.meta)?;
        crate::updates::flush_bgdt(self.io, &allocator, self.meta, &[2])?;

        self.io.flush()?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> ExtFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExtMeta) -> Self {
        Self { io, meta }
    }
}

// Tests have been moved/need update.
// Previously tests were inline. Since we drastically changed architecture,
// inline tests in `formatter.rs` relying on internal methods (which are gone) will break.
// However, the existing tests used `ExtFormatter::new(...).format(...)` which is the public API.
// So they should arguably still work if I keep them!
// Let's copy the tests back but ensure they work with new structure.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constant::*;
    use crate::group_layout::GroupLayout;
    // We need MemRimIO which is likely in rimio or tests utils.
    // rimio::prelude usually exports it.

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    fn make_meta_32mb() -> ExtMeta {
        ExtMeta::new(SIZE_BYTES, Some("TESTEXT"))
    }

    #[test]
    fn test_ext4_formatter_integration() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        // Run the orchestrator
        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT format failed");

        // Verify Superblock
        let mut sb = [0u8; 1024];
        io.read_at(EXT_SUPERBLOCK_OFFSET, &mut sb).unwrap();
        // Magic at 0x38 (56)
        let magic = u16::from_le_bytes(sb[56..58].try_into().unwrap());
        assert_eq!(magic, EXT_SUPERBLOCK_MAGIC, "Superblock magic mismatch");

        // Verify Root Directory Inode (Inode 2)
        // Group 0 Inode Table
        let layout = GroupLayout::compute(&meta, 0);
        let inode_table_offset = layout.inode_table_block as u64 * meta.block_size as u64;
        let root_inode_offset =
            inode_table_offset + (EXT_ROOT_INODE as u64 - 1) * EXT_DEFAULT_INODE_SIZE as u64;

        let mut inode_buf = [0u8; EXT_DEFAULT_INODE_SIZE as usize];
        io.read_at(root_inode_offset, &mut inode_buf).unwrap();

        // Mode is at offset 0, should be directory (0x4000)
        let mode = u16::from_le_bytes(inode_buf[0..2].try_into().unwrap());
        assert_eq!(mode & 0xF000, 0x4000, "Root inode is not a directory");

        println!("✓ Feature-based formatter integration test passed");
    }
}
