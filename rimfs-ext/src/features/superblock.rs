// SPDX-License-Identifier: MIT

//! ext4 primary and backup superblock serialization.

#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::ExtMeta;
use crate::types::{ExtSuperblock, GroupLayout};
use crate::{constant::*, utils::is_sparse_super_group};

#[derive(Default)]
pub struct ExtSuperblockFeature {
    sb_data: Option<ExtSuperblock>,
    sparse_offsets: Vec<u64>,
}

impl ExtSuperblockFeature {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<A, IO: RimIO + ?Sized> FsSystemFeature<ExtMeta, A, IO> for ExtSuperblockFeature {
    fn name(&self) -> &str {
        "Superblock"
    }

    fn prepare(&mut self, meta: &ExtMeta) -> FsFeatureResult<()> {
        let mut used_blocks: u32 = 0;
        let mut used_inodes: u32 = 0;

        for group in 0..meta.group_count {
            let layout = GroupLayout::compute(meta, group);
            let metadata_blocks = layout.metadata_blocks();
            let data_blocks_used = if group == 0 { 2 } else { 0 };
            used_blocks += metadata_blocks + data_blocks_used;

            let group_used_inodes = if group == 0 { 11 } else { 0 };
            used_inodes += group_used_inodes;
        }

        let sb = ExtSuperblock::from_meta(meta, used_blocks, used_inodes);
        self.sb_data = Some(sb);

        self.sparse_offsets.clear();
        for group_id in 0..meta.group_count {
            let has_super = !meta.features.has_sparse_super || is_sparse_super_group(group_id);

            if has_super && group_id != 0 {
                let group_start_block =
                    meta.first_data_block as u64 + group_id as u64 * meta.blocks_per_group as u64;
                let sb_copy_offset = group_start_block
                    .checked_mul(meta.block_size as u64)
                    .ok_or(crate::core::errors::FsFeatureError::InvalidConfiguration(
                        "EXT sparse superblock offset overflow",
                    ))?;
                self.sparse_offsets.push(sb_copy_offset);
            }
        }

        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut A) -> FsFeatureResult<()> {
        // Fixed allocation
        Ok(())
    }

    fn write(&self, io: &mut IO, _allocator: &A) -> FsFeatureResult<()> {
        let buf = self
            .sb_data
            .as_ref()
            .ok_or(crate::core::errors::FsFeatureError::NotPrepared)?;

        io.write_struct(EXT_SUPERBLOCK_OFFSET, buf)?;

        for offset in &self.sparse_offsets {
            io.write_struct(*offset, buf)?;
        }

        Ok(())
    }
}
