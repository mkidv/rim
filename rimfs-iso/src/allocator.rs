// SPDX-License-Identifier: MIT

use crate::meta::IsoMeta;
use crate::types::IsoHandle;
use rimfs_core::allocator::{FsAllocator, FsAllocatorResult};
use rimio::RimIO;

/// Sector allocator for ISO 9660 image generation.
pub struct IsoAllocator<'a> {
    _meta: &'a IsoMeta,
    current_sector: u64,
}

impl<'a> IsoAllocator<'a> {
    pub fn new(meta: &'a IsoMeta) -> Self {
        Self {
            _meta: meta,
            current_sector: 20, // Start after fixed system descriptors
        }
    }
}

impl<'a> FsAllocator<IsoHandle> for IsoAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<IsoHandle> {
        let handle = IsoHandle(self.current_sector);
        self.current_sector = self.current_sector.saturating_add(count as u64);
        Ok(handle)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<IsoHandle> {
        let handle = IsoHandle(self.current_sector);
        self.current_sector = self.current_sector.saturating_add(count as u64);
        Ok(handle)
    }

    fn used_units(&self) -> usize {
        self.current_sector as usize
    }

    fn remaining_units(&self) -> usize {
        usize::MAX.saturating_sub(self.used_units())
    }
}
