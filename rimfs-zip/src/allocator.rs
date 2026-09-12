// SPDX-License-Identifier: MIT

//! ZIP archive stream offset allocator.

use crate::meta::ZipMeta;
use crate::types::ZipHandle;
use rimfs_core::allocator::linear_allocator::LinearAllocator;
use rimfs_core::allocator::{FsAllocator, FsAllocatorResult};
use rimio::RimIO;

/// Offset allocator for ZIP archive stream writing.
pub struct ZipAllocator<'a> {
    _meta: &'a ZipMeta,
    inner: LinearAllocator,
}

impl<'a> ZipAllocator<'a> {
    pub fn new(meta: &'a ZipMeta) -> Self {
        Self {
            _meta: meta,
            inner: LinearAllocator::new(0, u64::MAX),
        }
    }
}

impl<'a> FsAllocator<ZipHandle> for ZipAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<ZipHandle> {
        self.inner.allocate(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<ZipHandle> {
        self.inner.allocate_contiguous(io, count)
    }

    fn used_units(&self) -> u64 {
        self.inner.current
    }

    fn remaining_units(&self) -> u64 {
        self.inner.remaining_units()
    }
}
