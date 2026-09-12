// SPDX-License-Identifier: MIT

//! FAT table cluster mapping feature.

use crate::allocator::FatAllocator;
use crate::core::errors::FsFeatureResult;
use crate::core::fat::FatDriver;
use crate::core::feature::FsSystemFeature;
use crate::meta::FatMeta;
use rimio::prelude::*;

/// Table feature responsible for initializing the primary and backup FAT tables.
#[derive(Debug, Default, Clone, Copy)]
pub struct FatTableFeature;

impl FatTableFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<FatMeta, FatAllocator<'a>, IO> for FatTableFeature {
    fn name(&self) -> &str {
        "FatTable"
    }

    fn prepare(&mut self, _meta: &FatMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, _io: &mut IO, _allocator: &mut FatAllocator<'a>) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &FatAllocator<'a>) -> FsFeatureResult<()> {
        let mut driver = FatDriver::new(allocator.meta);
        driver.format(io)?;
        Ok(())
    }
}
