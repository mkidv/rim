// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

pub use crate::core::allocator::*;

use crate::fs::fat32::meta::*;

#[derive(Debug, Clone)]
pub struct Fat32Handle {
    pub cluster_id: u32,
    pub cluster_chain: Vec<u32>,
}

impl Fat32Handle {
    pub fn new(cluster_id: u32) -> Self {
        Fat32Handle {
            cluster_id,
            cluster_chain: vec![cluster_id],
        }
    }

    pub fn from_chain(cluster_chain: Vec<u32>) -> Self {
        let cluster_id = *cluster_chain.first().expect("FAT32: empty cluster_chain");
        Self {
            cluster_id,
            cluster_chain,
        }
    }
}

impl FsHandle for Fat32Handle {}

#[derive(Debug, Clone)]
pub struct Fat32Allocator<'a> {
    meta: &'a Fat32Meta,
    next_free: u32,
}

impl<'a> Fat32Allocator<'a> {
    pub fn new(meta: &'a Fat32Meta) -> Self {
        Self {
            meta,
            next_free: meta.first_data_unit(),
        }
    }
}

impl<'a> FsAllocator<Fat32Handle> for Fat32Allocator<'a> {
    fn allocate_chain(&mut self, count: usize) -> FsAllocatorResult<Fat32Handle> {
         let mut chain = vec![0u32; count];
        for cluster in &mut chain {
            let next_cluster = self.next_free;
            if next_cluster > self.meta.last_data_unit() {
                return Err(FsAllocatorError::OutOfBlocks);
            }
            self.next_free += 1;
            *cluster = next_cluster;
        }
        Ok(Fat32Handle::from_chain(chain))
    }

    fn used_units(&self) -> usize {
        (self.next_free - self.meta.first_data_unit()) as usize
    }

    fn remaining_units(&self) -> usize {
        self.meta.total_units() - self.used_units()
    }
}
