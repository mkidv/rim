// SPDX-License-Identifier: MIT

//! FAT32 FSINFO sector allocation and synchronization.

use crate::allocator::FatAllocator;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::FatMeta;
use crate::types::FatFsInfo;
use rimio::prelude::*;

/// FSINFO feature responsible for primary and backup FSINFO sectors in FAT32.
#[derive(Debug, Default, Clone, Copy)]
pub struct FatFsInfoFeature;

impl FatFsInfoFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<FatMeta, FatAllocator<'a>, IO> for FatFsInfoFeature {
    fn name(&self) -> &str {
        "FSINFO"
    }

    fn prepare(&mut self, _meta: &FatMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut FatAllocator<'a>) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &FatAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        if meta.bits != 32 {
            return Ok(());
        }
        let fsinfo = FatFsInfo::from_meta(meta);

        let offset = FAT_FSINFO_SECTOR * meta.bytes_per_sector as u64;
        io.write_struct(offset, &fsinfo)?;

        let backup_offset = FAT_FSINFO_BACKUP_SECTOR * meta.bytes_per_sector as u64;
        io.write_struct(backup_offset, &fsinfo)?;

        Ok(())
    }
}
