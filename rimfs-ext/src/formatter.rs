// SPDX-License-Identifier: MIT

//! ext2/3/4 filesystem volume formatter.

use rimio::prelude::*;

use crate::allocator::ExtAllocator;
use crate::core::feature::{FsSystemFeature, execute_feature_pipeline};
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

        let mut superblock = ExtSuperblockFeature::new();
        let mut block_group = BlockGroupFeature::new();
        let mut inode_table = InodeTableFeature::new();
        let mut root_dir = RootDirFeature::new();
        let mut lost_found = LostFoundFeature::new();

        let mut features: [&mut dyn FsSystemFeature<ExtMeta, ExtAllocator<'a>, IO>; 5] = [
            &mut superblock,
            &mut block_group,
            &mut inode_table,
            &mut root_dir,
            &mut lost_found,
        ];

        let mut allocator = ExtAllocator::new(self.meta);
        execute_feature_pipeline(&mut features, self.meta, &mut allocator, self.io)?;

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
