// SPDX-License-Identifier: MIT

//! FAT boot sector feature and VBR writer.

use crate::allocator::FatAllocator;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::FatMeta;
use crate::types::FatVbr;
use rimio::prelude::*;

/// Boot feature responsible for VBR, backup VBR, and FAT32 boot code sectors.
#[derive(Debug, Default, Clone, Copy)]
pub struct FatBootFeature;

impl FatBootFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<FatMeta, FatAllocator<'a>, IO> for FatBootFeature {
    fn name(&self) -> &str {
        "Boot"
    }

    fn prepare(&mut self, _meta: &FatMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut FatAllocator<'a>) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &FatAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let mut vbr = FatVbr::from_meta(meta);
        let bps = meta.bytes_per_sector as u64;

        let hidden_sectors = (io.partition_offset() / bps) as u32;
        vbr = vbr.with_hidden_sectors(hidden_sectors);

        if meta.bits == 32 {
            vbr = vbr.with_boot_code(32, &boot_code::FAT32_BOOT_CODE_P0);
        }

        let offset = FAT_VBR_SECTOR * bps;
        io.write_struct(offset, &vbr)?;

        let backup_offset = FAT_VBR_BACKUP_SECTOR * bps;
        io.write_struct(backup_offset, &vbr)?;

        if meta.bits == 32 {
            let mut p2 = [0u8; 512];
            p2[0..510].copy_from_slice(&boot_code::FAT32_BOOT_CODE_P2);
            p2[510..512].copy_from_slice(&FAT_SIGNATURE.to_le_bytes());
            io.write_at(2 * bps, &p2)?;
            io.write_at(8 * bps, &p2)?;
        }

        Ok(())
    }
}
