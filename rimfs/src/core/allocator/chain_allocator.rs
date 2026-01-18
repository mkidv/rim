// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::core::allocator::{FsAllocator, FsAllocatorError, FsAllocatorResult, FsHandle};
use crate::core::meta::FsMeta;

/// A generic allocator for simple cluster chains (FAT-like).
///
/// Maintains a `next_free` cursor and blindly allocates the next available clusters
/// until it reaches the end of the data region.
#[derive(Debug, Clone, Copy)]
pub struct ChainAllocator<'a, M> {
    pub meta: &'a M,
    pub next_free: u32,
}

impl<'a, M: FsMeta<u32>> ChainAllocator<'a, M> {
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

impl<'a, M, H> FsAllocator<H> for ChainAllocator<'a, M>
where
    M: FsMeta<u32>,
    H: FsHandle + From<Vec<u32>> + Clone,
{
    fn allocate_chain(&mut self, count: usize) -> FsAllocatorResult<H> {
        let mut chain = vec![0u32; count];
        for cluster in &mut chain {
            let next_cluster = self.next_free;
            if next_cluster > self.meta.last_data_unit() {
                return Err(FsAllocatorError::OutOfBlocks);
            }
            self.next_free += 1;
            *cluster = next_cluster;
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
