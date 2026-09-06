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
        allocator.flush_superblock(self.io, self.meta)?;
        allocator.flush_bgdt(self.io, self.meta, &[2])?;

        self.io.flush()?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> ExtFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExtMeta) -> Self {
        Self { io, meta }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::ExtChecker;
    use crate::constant::*;
    use crate::types::GroupLayout;
    use rimfs_core::checker::{FsChecker, VerifyReport};
    use rimfs_core::testing::assert_has_error;

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    fn make_meta_32mb() -> ExtMeta {
        ExtMeta::new(SIZE_BYTES, Some("TESTEXT")).unwrap()
    }

    #[test]
    fn test_ext4_formatter_integration() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT format failed");

        let mut sb = [0u8; 1024];
        io.read_at(EXT_SUPERBLOCK_OFFSET, &mut sb).unwrap();
        let magic = u16::from_le_bytes(sb[56..58].try_into().unwrap());
        assert_eq!(magic, EXT_SUPERBLOCK_MAGIC, "Superblock magic mismatch");

        let layout = GroupLayout::compute(&meta, 0);
        let inode_table_offset = layout.inode_table_block * meta.block_size as u64;
        let root_inode_offset =
            inode_table_offset + (EXT_ROOT_INODE as u64 - 1) * EXT_DEFAULT_INODE_SIZE as u64;

        let mut inode_buf = [0u8; EXT_DEFAULT_INODE_SIZE as usize];
        io.read_at(root_inode_offset, &mut inode_buf).unwrap();

        let mode = u16::from_le_bytes(inode_buf[0..2].try_into().unwrap());
        assert_eq!(mode & 0xF000, 0x4000, "Root inode is not a directory");
    }

    #[test]
    fn test_ext_superblock_corruption_detection() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT format failed");

        io.write_at(EXT_SUPERBLOCK_OFFSET + 0x38, &[0x00, 0x00])
            .unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let mut report = VerifyReport::default();
        checker
            .check_boot(&Default::default(), &mut report)
            .unwrap();
        assert_has_error(&report, "SB.MAGIC");
    }
}
