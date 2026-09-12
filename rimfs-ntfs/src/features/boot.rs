// SPDX-License-Identifier: MIT
//! NTFS Boot Sector Feature ($Boot)
//!
//! Handles writing the primary Volume Boot Record (VBR) at sector 0
//! and the backup VBR at the last sector of the volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;

use rimio::prelude::*;

use crate::allocator::NtfsAllocator;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::NtfsMeta;
use crate::types::record::NtfsBootSector;
use zerocopy::IntoBytes;

#[derive(Default)]
pub struct NtfsBootFeature {
    backup_offset: u64,
    sector_size: usize,
}

impl NtfsBootFeature {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO> for NtfsBootFeature {
    fn name(&self) -> &str {
        "NTFS Boot Sector"
    }

    fn prepare(&mut self, meta: &NtfsMeta) -> FsFeatureResult<()> {
        self.backup_offset = meta.backup_boot_sector_offset();
        self.sector_size = meta.bytes_per_sector as usize;
        if self.sector_size < core::mem::size_of::<NtfsBootSector>() {
            return Err(crate::core::errors::FsFeatureError::InvalidConfiguration(
                "NTFS sector is smaller than its boot record",
            ));
        }
        Ok(())
    }

    fn allocate(
        &mut self,
        _io: &mut IO,
        _allocator: &mut NtfsAllocator<'a>,
    ) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &NtfsAllocator<'a>) -> FsFeatureResult<()> {
        // 1. Write primary boot sector at sector 0
        let boot = NtfsBootSector::new_from_meta(allocator.meta);
        io.write_struct(0, &boot)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 2. Write backup boot sector at last sector of the volume
        if self.sector_size == core::mem::size_of::<NtfsBootSector>() {
            io.write_struct(self.backup_offset, &boot)?;
        } else {
            // Preserve the containing sector's tail without rereading the fixed boot record.
            let mut boot_copy = vec![0u8; self.sector_size];
            let size = core::mem::size_of::<NtfsBootSector>();
            boot_copy[..size].copy_from_slice(boot.as_bytes());
            io.read_at(size as u64, &mut boot_copy[size..])?;
            io.write_at(self.backup_offset, &boot_copy)?;
        }

        Ok(())
    }
}
