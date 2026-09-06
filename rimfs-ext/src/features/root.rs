// SPDX-License-Identifier: MIT

#[cfg(not(feature = "std"))]
use alloc::vec;

use rimio::prelude::*;

use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::core::traits::FileAttributes;
use crate::meta::ExtMeta;
use crate::types::{ExtDirEntry, ExtExtent, ExtInode};
use crate::{constant::*, types::GroupLayout};

#[derive(Default)]
pub struct RootDirFeature {
    block_size: u32,
    root_block: u32,
    inode_offset: u64,
    block_offset: u64,
    inode_size: u32,
}

impl RootDirFeature {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<A, IO: RimIO + ?Sized> FsSystemFeature<ExtMeta, A, IO> for RootDirFeature {
    fn name(&self) -> &str {
        "Root Directory"
    }

    fn prepare(&mut self, meta: &ExtMeta) -> FsFeatureResult<()> {
        self.block_size = meta.block_size;
        self.inode_size = meta.inode_size;

        let layout = GroupLayout::compute(meta, 0);
        self.root_block = u32::try_from(layout.first_data_block).map_err(|_| {
            crate::core::errors::FsFeatureError::InvalidConfiguration(
                "EXT root block exceeds extent address range",
            )
        })?;
        self.block_offset = self.root_block as u64 * meta.block_size as u64;

        let inode_table_block = layout.inode_table_block;
        let inode_table_offset = inode_table_block * meta.block_size as u64;
        self.inode_offset =
            inode_table_offset + (EXT_ROOT_INODE as u64 - 1) * meta.inode_size as u64;

        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut A) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, _allocator: &A) -> FsFeatureResult<()> {
        // 1. Write Directory Block
        let mut dir_buf = vec![];
        ExtDirEntry::dot(EXT_ROOT_INODE).to_raw_buffer(&mut dir_buf);
        ExtDirEntry::dotdot(EXT_ROOT_INODE).to_raw_buffer(&mut dir_buf);
        ExtLostFound::entry().to_raw_buffer(&mut dir_buf);

        // Pad
        while dir_buf.len() < self.block_size as usize {
            dir_buf.push(0);
        }

        io.write_at(self.block_offset, &dir_buf)?;

        // 2. Write Inode
        // We need to construct the Inode on the fly.
        // We know the extent points to `root_block`.
        let extent = ExtExtent::new(0, self.root_block, 1);
        let root_inode = ExtInode::from_attr(
            &FileAttributes::new_dir(),
            self.block_size as u64,
            EXT_ROOT_DIR_LINKS_COUNT + 1, // +1 for lost+found
            self.block_size.div_ceil(512),
            &[extent],
        );
        let root_inode_buf = root_inode.to_bytes();

        io.write_at(
            self.inode_offset,
            &root_inode_buf[..self.inode_size as usize],
        )?;

        Ok(())
    }
}

// Needed to reference LostFound entry creation
use crate::types::ExtLostFound;
