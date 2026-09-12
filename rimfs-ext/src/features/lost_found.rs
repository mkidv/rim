// SPDX-License-Identifier: MIT

//! ext4 lost+found directory creation.

use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::ExtMeta;
use crate::types::ExtLostFound;
use crate::types::GroupLayout;
use rimio::RimIO;

#[derive(Default)]
pub struct LostFoundFeature {
    block_size: u32,
    lf_block: u32,
    inode_offset: u64,
    block_offset: u64,
    inode_size: u32,
    has_extents: bool,
}

impl LostFoundFeature {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<A, IO: RimIO + ?Sized> FsSystemFeature<ExtMeta, A, IO> for LostFoundFeature {
    fn name(&self) -> &str {
        "Lost+Found Directory"
    }

    fn prepare(&mut self, meta: &ExtMeta) -> FsFeatureResult<()> {
        self.block_size = meta.block_size;
        self.inode_size = meta.inode_size;
        self.has_extents = meta.features.has_extents;

        let layout = GroupLayout::compute(meta, 0);
        // lost+found is at first_data_block + 1 (root is at first_data_block)
        self.lf_block = u32::try_from(layout.first_data_block + 1).map_err(|_| {
            crate::core::errors::FsFeatureError::InvalidConfiguration(
                "EXT lost+found block exceeds extent address range",
            )
        })?;
        self.block_offset = self.lf_block as u64 * meta.block_size as u64;

        let inode_table_block = layout.inode_table_block;
        let inode_table_offset = inode_table_block * meta.block_size as u64;
        self.inode_offset =
            inode_table_offset + (ExtLostFound::INODE as u64 - 1) * meta.inode_size as u64;

        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut A) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, _allocator: &A) -> FsFeatureResult<()> {
        // 1. Write Directory Block
        let dir_buf = ExtLostFound::create_dir_block(self.block_size as usize);
        io.write_at(self.block_offset, &dir_buf)?;

        // 2. Write Inode
        let mut inode_data = ExtLostFound::create_inode(self.block_size, self.lf_block);
        if !self.has_extents {
            let mut map = crate::types::BlockMapArray::default();
            map.direct[0] = self.lf_block.into();
            inode_data.set_block_map(&map);
        }
        let inode_buf = inode_data.to_bytes();
        io.write_at(self.inode_offset, &inode_buf[..self.inode_size as usize])?;

        Ok(())
    }
}
