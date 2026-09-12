// SPDX-License-Identifier: MIT

//! ISO 9660 sector and block allocator.

use crate::meta::IsoMeta;
use crate::types::IsoHandle;
use rimfs_core::allocator::linear_allocator::LinearAllocator;
use rimfs_core::allocator::{FsAllocator, FsAllocatorResult};
use rimio::RimIO;

/// Sector allocator for ISO 9660 image generation.
pub struct IsoAllocator<'a> {
    _meta: &'a IsoMeta,
    inner: LinearAllocator,
}

impl<'a> IsoAllocator<'a> {
    pub fn new(meta: &'a IsoMeta) -> Self {
        Self {
            _meta: meta,
            inner: LinearAllocator::new(20, u64::MAX),
        }
    }
}

impl<'a> FsAllocator<IsoHandle> for IsoAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<IsoHandle> {
        self.inner.allocate(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<IsoHandle> {
        self.inner.allocate_contiguous(io, count)
    }

    fn used_units(&self) -> u64 {
        self.inner.current
    }

    fn remaining_units(&self) -> u64 {
        self.inner.remaining_units()
    }
}
