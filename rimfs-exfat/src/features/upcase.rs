// SPDX-License-Identifier: MIT

//! exFAT up-case table cluster allocation.

use crate::allocator::ExFatAllocator;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::core::traits::FsMeta;
use crate::meta::ExFatMeta;
use crate::upcase::UpcaseHandle;
use rimio::prelude::*;

/// Upcase feature responsible for writing the case-folding table.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExFatUpcaseFeature;

impl ExFatUpcaseFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<ExFatMeta, ExFatAllocator<'a>, IO>
    for ExFatUpcaseFeature
{
    fn name(&self) -> &str {
        "Upcase"
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
        let offset = meta.unit_offset(meta.upcase_cluster);
        let upcase = UpcaseHandle::from_flavor(&meta.upcase_flavor);

        io.write_block_best_effort(offset, upcase.as_bytes(), meta.unit_size() as usize)?;

        Ok(())
    }
}
