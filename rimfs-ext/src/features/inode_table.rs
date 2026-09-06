// SPDX-License-Identifier: MIT

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::ExtMeta;
use crate::types::GroupLayout;
use rimio::prelude::*;

struct InodeTableRegion {
    offset: u64,
    size: usize,
}

#[derive(Default)]
pub struct InodeTableFeature {
    regions: Vec<InodeTableRegion>,
}

impl InodeTableFeature {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<A, IO: RimIO + ?Sized> FsSystemFeature<ExtMeta, A, IO> for InodeTableFeature {
    fn name(&self) -> &str {
        "Inode Tables"
    }

    fn prepare(&mut self, meta: &ExtMeta) -> FsFeatureResult<()> {
        let inode_table_size = (meta.inode_size * meta.inodes_per_group) as usize;
        self.regions.clear();

        for group in 0..meta.group_count {
            let layout = GroupLayout::compute(meta, group);
            let inode_table_block = layout.inode_table_block;
            let offset = inode_table_block * meta.block_size as u64;

            self.regions.push(InodeTableRegion {
                offset,
                size: inode_table_size,
            });
        }

        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut A) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, _allocator: &A) -> FsFeatureResult<()> {
        for region in &self.regions {
            io.zero_fill(region.offset, region.size)?;
        }
        Ok(())
    }
}
