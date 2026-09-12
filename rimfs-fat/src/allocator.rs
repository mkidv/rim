// SPDX-License-Identifier: MIT

//! FAT cluster allocator and free-chain tracker.

pub use crate::core::allocator::*;
use alloc::vec::Vec;
use rimio::prelude::*;

use crate::core::fat::*;
use crate::meta::*;

#[derive(Debug, Clone)]
pub struct FatHandle {
    pub cluster_id: u32,
    pub cluster_chain: RunList,
}

impl FatHandle {
    pub fn new(cluster_id: u32) -> Self {
        FatHandle {
            cluster_id,
            cluster_chain: RunList::from_units(&[cluster_id]),
        }
    }

    /// Creates a handle from an existing cluster chain.
    pub fn from_chain(cluster_chain: RunList) -> Self {
        let cluster_id = cluster_chain.get_unit(0).map(|u| u as u32).unwrap_or(0);
        Self {
            cluster_id,
            cluster_chain,
        }
    }
}

impl From<RunList> for FatHandle {
    fn from(chain: RunList) -> Self {
        Self::from_chain(chain)
    }
}

impl From<Vec<u32>> for FatHandle {
    fn from(chain: Vec<u32>) -> Self {
        Self::from_chain(RunList::from_units(&chain))
    }
}

impl FsHandle for FatHandle {}

/// Real FAT Allocator that scans the FAT table via IO.
pub struct FatAllocator<'a> {
    pub meta: &'a FatMeta,
    pub next_free_hint: u32,
    pub free_count: u64,
}

impl<'a> FatAllocator<'a> {
    pub fn new(meta: &'a FatMeta) -> Self {
        Self {
            meta,
            next_free_hint: meta.first_data_unit() + meta.system_used_clusters(),
            free_count: u64::from(
                meta.cluster_count
                    .saturating_sub(meta.system_used_clusters()),
            ),
        }
    }

    /// Initializes the allocator by scanning for the first free cluster on disk.
    /// This optimization prevents re-scanning used clusters on every allocation.
    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO, meta: &'a FatMeta) -> FsAllocatorResult<Self> {
        let mut allocator = Self::new(meta);

        // Efficiently scan for free clusters
        let (free_count, next_hint) = FatDriver::new(meta).find_next_free(io)?;

        allocator.free_count = free_count as u64;
        allocator.next_free_hint = next_hint;

        Ok(allocator)
    }
}

impl<'a> FsAllocator<FatHandle> for FatAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<FatHandle> {
        crate::ensure!(count > 0, FsAllocatorError::InvalidSize);

        // Prefer contiguous first
        match self.allocate_contiguous(io, count) {
            Ok(handle) => return Ok(handle),
            Err(FsAllocatorError::OutOfBlocks) => {}
            Err(e) => return Err(e),
        }

        let mut driver = FatDriver::new(self.meta);
        let chain = driver
            .find_free_runs(io, self.next_free_hint, count)?
            .ok_or(FsAllocatorError::OutOfBlocks)?;

        driver.write_run_list(io, &chain)?;
        self.free_count = self.free_count.saturating_sub(count);
        if let Some(last_run) = chain.0.last() {
            let next_hint = (last_run.start + last_run.length) as u32;
            self.next_free_hint = if next_hint > self.meta.last_data_unit() {
                self.meta.first_data_unit()
            } else {
                next_hint
            };
        }

        Ok(FatHandle::from(chain))
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<FatHandle> {
        crate::ensure!(count > 0, FsAllocatorError::InvalidSize);

        let count_u32 = u32::try_from(count).map_err(|_| FsAllocatorError::InvalidSize)?;
        let mut driver = FatDriver::new(self.meta);

        // Search from hint, wrap to beginning if needed
        let range_start = match driver.find_next_free_run(io, self.next_free_hint, count_u32)? {
            Some(start) => start,
            None if self.next_free_hint != self.meta.first_data_unit() => driver
                .find_next_free_run(io, self.meta.first_data_unit(), count_u32)?
                .ok_or(FsAllocatorError::OutOfBlocks)?,
            None => return Err(FsAllocatorError::OutOfBlocks),
        };

        let mut chain = RunList::new();
        chain.push(rimio::run::Run {
            start: range_start as u64,
            length: count,
        });
        driver.write_run_list(io, &chain)?;

        self.free_count = self.free_count.saturating_sub(count);
        let next_hint = range_start + count_u32;
        self.next_free_hint = if next_hint > self.meta.last_data_unit() {
            self.meta.first_data_unit()
        } else {
            next_hint
        };

        Ok(FatHandle::from(chain))
    }

    fn used_units(&self) -> u64 {
        (self.meta.cluster_count as u64).saturating_sub(self.free_count)
    }

    fn remaining_units(&self) -> u64 {
        self.free_count
    }
}
