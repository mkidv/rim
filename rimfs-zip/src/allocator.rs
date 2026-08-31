// SPDX-License-Identifier: MIT

use crate::meta::ZipMeta;
use crate::types::ZipHandle;
use rimfs_core::allocator::{FsAllocator, FsAllocatorResult};
use rimio::RimIO;

/// Offset allocator for ZIP archive stream writing.
pub struct ZipAllocator<'a> {
    _meta: &'a ZipMeta,
    current_offset: u64,
}

impl<'a> ZipAllocator<'a> {
    pub fn new(meta: &'a ZipMeta) -> Self {
        Self {
            _meta: meta,
            current_offset: 0,
        }
    }
}

impl<'a> FsAllocator<ZipHandle> for ZipAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<ZipHandle> {
        let handle = ZipHandle(self.current_offset);
        self.current_offset = self.current_offset.saturating_add(count as u64);
        Ok(handle)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<ZipHandle> {
        let handle = ZipHandle(self.current_offset);
        self.current_offset = self.current_offset.saturating_add(count as u64);
        Ok(handle)
    }

    fn used_units(&self) -> usize {
        self.current_offset as usize
    }

    fn remaining_units(&self) -> usize {
        usize::MAX.saturating_sub(self.used_units())
    }
}
