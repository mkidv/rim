// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

pub use crate::core::allocator::*;

use crate::fs::exfat::meta::*;

pub use crate::core::allocator::chain_allocator::ChainAllocator;

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

    /// Creates a handle from an existing cluster chain.
    ///
    /// If the chain is empty, `cluster_id` defaults to 0 (invalid cluster),
    /// which should be caught by validation logic elsewhere.
    pub fn from_chain(cluster_chain: Vec<u32>) -> Self {
        let cluster_id = cluster_chain.first().copied().unwrap_or(0);
        Self {
            cluster_id,
            cluster_chain,
        }
    }
}

impl From<Vec<u32>> for ExFatHandle {
    fn from(chain: Vec<u32>) -> Self {
        Self::from_chain(chain)
    }
}

impl FsHandle for ExFatHandle {}

pub type ExFatAllocator<'p> = ChainAllocator<'p, ExFatMeta>;
