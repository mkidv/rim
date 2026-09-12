// SPDX-License-Identifier: MIT

//! exFAT allocation bitmap cluster allocation and writing.

use crate::allocator::ExFatAllocator;
use crate::core::bitmap::BitmapDriver;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::ExFatMeta;
use rimio::prelude::*;

/// Bitmap feature responsible for formatting the allocation bitmap and setting initial system clusters.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExFatBitmapFeature;

impl ExFatBitmapFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<ExFatMeta, ExFatAllocator<'a>, IO>
    for ExFatBitmapFeature
{
    fn name(&self) -> &str {
        "Bitmap"
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
        let mut bd = BitmapDriver::new(meta);
        bd.format_with(io, 0x00)?;
        bd.set_bits_range(io, 0, meta.system_used_clusters() as u64, true)?;
        bd.flush(io)?;
        Ok(())
    }
}
