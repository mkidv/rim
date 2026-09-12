// SPDX-License-Identifier: MIT

//! exFAT FAT table cluster mapping.

use crate::allocator::ExFatAllocator;
use crate::core::errors::FsFeatureResult;
use crate::core::fat::FatDriver;
use crate::core::feature::FsSystemFeature;
use crate::meta::ExFatMeta;
use rimio::prelude::*;

/// FAT table feature responsible for formatting the exFAT allocation table.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExFatFatFeature;

impl ExFatFatFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<ExFatMeta, ExFatAllocator<'a>, IO>
    for ExFatFatFeature
{
    fn name(&self) -> &str {
        "FAT"
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
        let mut fd = FatDriver::new(allocator.meta);
        fd.format(io)?;
        Ok(())
    }
}
