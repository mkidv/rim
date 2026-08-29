// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::RimIO;

use crate::allocator::{FsAllocator, FsAllocatorError, FsAllocatorResult, FsHandle};
use crate::meta::FsMeta;

/// A simple linear allocator (bump allocator).
///
/// Maintains a `next_free` cursor and blindly allocates the next available units
/// until it reaches the end of the data region.
///
/// This is primarily useful for:
/// - Initial formatting of a filesystem (writing sequentially).
/// - Simple filesystems where fragmentation is not a concern or handled elsewhere.
#[derive(Debug, Clone, Copy)]
pub struct LinearAllocator<'a, M> {
    pub meta: &'a M,
    pub next_free: u32,
}

impl<'a, M: FsMeta<u32>> LinearAllocator<'a, M> {
    pub fn new(meta: &'a M) -> Self {
        Self {
            meta,
            next_free: meta.first_data_unit(),
        }
    }

    pub fn used_units(&self) -> usize {
        (self.next_free - self.meta.first_data_unit()) as usize
    }

    pub fn remaining_units(&self) -> usize {
        self.meta.total_units() - self.used_units()
    }
}

impl<'a, M, H> FsAllocator<H> for LinearAllocator<'a, M>
where
    M: FsMeta<u32>,
    H: FsHandle + From<Vec<u32>> + Clone,
{
    fn allocate<IO: RimIO + ?Sized>(&mut self, io: &mut IO, count: usize) -> FsAllocatorResult<H> {
        self.allocate_contiguous(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<H> {
        let mut chain = vec![0u32; count];
        for unit in &mut chain {
            let next_unit = self.next_free;
            crate::ensure!(
                next_unit <= self.meta.last_data_unit(),
                FsAllocatorError::OutOfBlocks
            );
            self.next_free += 1;
            *unit = next_unit;
        }
        Ok(H::from(chain))
    }

    fn used_units(&self) -> usize {
        (self.next_free - self.meta.first_data_unit()) as usize
    }

    fn remaining_units(&self) -> usize {
        self.meta.total_units() - ((self.next_free - self.meta.first_data_unit()) as usize)
    }
}
