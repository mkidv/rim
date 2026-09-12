// SPDX-License-Identifier: MIT

//! FAT root directory cluster initialization.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::allocator::FatAllocator;
use crate::core::errors::{FsFeatureError, FsFeatureResult};
use crate::core::fat::FatDriver;
use crate::core::feature::FsSystemFeature;
use crate::core::traits::FsMeta;
use crate::meta::FatMeta;
use crate::types::{FatEntries, FatEodEntry};
use rimio::prelude::*;

/// Root directory feature responsible for reserving and writing the initial root directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct FatRootDirFeature;

impl FatRootDirFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<FatMeta, FatAllocator<'a>, IO> for FatRootDirFeature {
    fn name(&self) -> &str {
        "RootDir"
    }

    fn prepare(&mut self, _meta: &FatMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, allocator: &mut FatAllocator<'a>) -> FsFeatureResult<()> {
        allocator.free_count = allocator.free_count.saturating_sub(1);
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &FatAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let root = meta.root_unit();

        // 1. Write the root cluster entry in the FAT tables
        let mut rl = RunList::new();
        rl.push(Run {
            start: root as u64,
            length: 1,
        });
        let mut driver = FatDriver::new(meta);
        driver
            .write_run_list(io, &rl)
            .map_err(FsFeatureError::from)?;

        // 2. Write the root directory cluster contents
        let mut buf = Vec::with_capacity(meta.unit_size() as usize);
        FatEntries::volume_label(meta.volume_label)
            .with_integrity_calculated(meta)
            .to_raw_buffer(&mut buf);
        FatEodEntry::new().to_raw_buffer(&mut buf);

        let offset = meta.unit_offset(root);
        io.write_at(offset, &buf)?;

        Ok(())
    }
}
