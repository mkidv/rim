// SPDX-License-Identifier: MIT

//! exFAT VBR, extended boot sectors, and checksum calculation.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::allocator::ExFatAllocator;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::core::utils::checksum_utils::{accumulate_checksum, accumulate_checksum_with_escape};
use crate::meta::ExFatMeta;
use crate::types::{ExFatBootSector, ExFatExBootSector};

/// Boot feature responsible for main and backup VBR regions (sectors 0..=23).
#[derive(Debug, Default, Clone, Copy)]
pub struct ExFatBootFeature;

impl ExFatBootFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<ExFatMeta, ExFatAllocator<'a>, IO>
    for ExFatBootFeature
{
    fn name(&self) -> &str {
        "Boot"
    }

    fn prepare(&mut self, _meta: &ExFatMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(
        &mut self,
        _io: &mut IO,
        _allocator: &mut ExFatAllocator<'a>,
    ) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &ExFatAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let mut buf = Vec::with_capacity(12 * meta.bytes_per_sector as usize);

        let partition_offset_sectors = io.partition_offset() / (meta.bytes_per_sector as u64);
        let mut checksum: u32 = 0;

        // Sector 0
        let vbr = ExFatBootSector::new_from_meta(meta)
            .with_partition_offset(partition_offset_sectors)
            .with_percent_in_use(meta.percent_in_use());
        vbr.to_raw_buffer(&mut buf);
        accumulate_checksum_with_escape(&mut checksum, vbr.as_bytes(), |i, _b| {
            i == 106 || i == 107 || i == 112
        });

        // Sectors 1-8: Extended Boot Sectors
        let ex = ExFatExBootSector::new();
        for i in 1..=8 {
            if i == 1 {
                buf.extend_from_slice(&EXFAT_EXT_BOOT_SECTOR_1);
                buf.extend_from_slice(&EXFAT_SIGNATURE);
                accumulate_checksum(&mut checksum, &EXFAT_EXT_BOOT_SECTOR_1);
                accumulate_checksum(&mut checksum, &EXFAT_SIGNATURE);
            } else if i == 2 {
                buf.extend_from_slice(&EXFAT_EXT_BOOT_SECTOR_2);
                buf.extend_from_slice(&EXFAT_SIGNATURE);
                accumulate_checksum(&mut checksum, &EXFAT_EXT_BOOT_SECTOR_2);
                accumulate_checksum(&mut checksum, &EXFAT_SIGNATURE);
            } else {
                ex.to_raw_buffer(&mut buf);
                accumulate_checksum(&mut checksum, ex.as_bytes());
            }
        }

        // Sectors 9-10: OEM Parameters and Reserved (without signature per spec)
        let empty = vec![0u8; meta.bytes_per_sector as usize];
        for _ in 9..=10 {
            buf.extend_from_slice(&empty);
            accumulate_checksum(&mut checksum, &empty);
        }

        // Sector 11: Checksum sector
        let sec = meta.bytes_per_sector as usize;
        let mut chk = vec![0u8; sec];
        for i in (0..sec).step_by(4) {
            chk[i..i + 4].copy_from_slice(&checksum.to_le_bytes());
        }
        buf.extend_from_slice(&chk);

        let offset = EXFAT_VBR_SECTOR * meta.bytes_per_sector as u64;
        io.write_at(offset, &buf)?;

        let backup_offset = EXFAT_VBR_BACKUP_SECTOR * meta.bytes_per_sector as u64;
        io.write_at(backup_offset, &buf)?;

        Ok(())
    }
}
