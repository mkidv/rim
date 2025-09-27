// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

pub use crate::core::allocator::*;

use crate::fs::exfat::meta::*;

#[derive(Debug, Clone)]
pub struct ExFatHandle {
    pub cluster_id: u32,
    pub cluster_chain: Vec<u32>,
}

impl ExFatHandle {
    pub fn new(cluster_id: u32) -> Self {
        ExFatHandle {
            cluster_id,
            cluster_chain: vec![cluster_id],
        }
    }

    pub fn from_chain(cluster_chain: Vec<u32>) -> Self {
        let cluster_id = *cluster_chain.first().expect("ExFat: empty cluster_chain");
        Self {
            cluster_id,
            cluster_chain,
        }
    }
}

impl FsHandle for ExFatHandle {}

#[derive(Debug, Clone, Copy)]
pub struct ExFatAllocator<'p> {
    meta: &'p ExFatMeta,
    next_free: u32,
}

impl<'p> ExFatAllocator<'p> {
    pub fn new(meta: &'p ExFatMeta) -> Self {
        Self {
            meta,
            next_free: meta.first_data_unit(),
        }
    }
}

impl<'p> FsAllocator<ExFatHandle> for ExFatAllocator<'p> {
    fn allocate_chain(&mut self, count: usize) -> FsAllocatorResult<ExFatHandle> {
        let mut chain = vec![0u32; count];
        for cluster in &mut chain {
            let next_cluster = self.next_free;
            if next_cluster > self.meta.last_data_unit() {
                return Err(FsAllocatorError::OutOfBlocks);
            }
            self.next_free += 1;
            *cluster = next_cluster;
        }
        Ok(ExFatHandle::from_chain(chain))
    }

    fn used_units(&self) -> usize {
        (self.next_free - self.meta.first_data_unit()) as usize
    }

    fn remaining_units(&self) -> usize {
        self.meta.total_units() - self.used_units()
    }
}
