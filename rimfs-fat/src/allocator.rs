pub use crate::core::allocator::*;
use alloc::vec::Vec;
use rimio::prelude::*;

use crate::core::fat::*;
use crate::meta::*;

// pub use crate::core::allocator::linear_allocator::LinearAllocator;

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
    pub free_count: usize,
}

impl<'a> FatAllocator<'a> {
    pub fn new(meta: &'a FatMeta) -> Self {
        Self {
            meta,
            next_free_hint: meta.first_data_unit() + meta.system_used_clusters(),
            free_count: meta
                .cluster_count
                .saturating_sub(meta.system_used_clusters()) as usize,
        }
    }

    /// Initializes the allocator by scanning for the first free cluster on disk.
    /// This optimization prevents re-scanning used clusters on every allocation.
    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO, meta: &'a FatMeta) -> FsAllocatorResult<Self> {
        let mut allocator = Self::new(meta);

        // Scan for the first free cluster
        // Efficiently scan for free clusters
        let (free_count, next_hint) = FatDriver::new(meta).find_next_free(io)?;

        allocator.free_count = free_count;
        allocator.next_free_hint = next_hint;

        Ok(allocator) // Return early as we scanned everything
    }
}

impl<'a> FsAllocator<FatHandle> for FatAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<FatHandle> {
        crate::ensure!(count > 0, FsAllocatorError::InvalidSize);

        let mut chain = RunList::new();
        let start = self.next_free_hint;
        let end = self.meta.last_data_unit();
        let wrap_limit = self.meta.first_data_unit();

        let mut current_search = start;
        let mut searched_count = 0;
        let total_units = (end - wrap_limit) + 1;

        let mut driver = FatDriver::new(self.meta);

        while chain.total_units() < count as u64 {
            crate::ensure!(searched_count <= total_units, FsAllocatorError::OutOfBlocks);

            // Check if cluster is free using buffered view
            let val = driver
                .get(io, current_search)
                .map_err(FsAllocatorError::IO)?;

            if val == 0 {
                // Found free cluster
                chain.push_unit(current_search as u64);
                self.free_count = self.free_count.saturating_sub(1);
            }

            // Advance search cursor
            current_search += 1;
            if current_search > end {
                current_search = self.meta.first_data_unit();
            }
            searched_count += 1;
        }

        // Update the hint to the next potential free block (optimization)
        self.next_free_hint = current_search;

        // Write the chain to disk
        driver.write_run_list(io, &chain)?;

        Ok(FatHandle::from(chain))
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<FatHandle> {
        crate::ensure!(count > 0, FsAllocatorError::InvalidSize);

        let start = self.next_free_hint;
        let end = self.meta.last_data_unit();
        let wrap_limit = self.meta.first_data_unit();

        let mut driver = FatDriver::new(self.meta);

        // Find contiguous range
        let mut searched_count = 0;
        let total_units = (end - wrap_limit) + 1;
        let mut current = start;

        while searched_count < total_units {
            let mut found_count = 0;
            let range_start = current;

            for i in 0..count {
                let check_unit = current + i as u32;
                if check_unit > end {
                    break;
                }

                let val = driver.get(io, check_unit).map_err(FsAllocatorError::IO)?;
                if val != 0 {
                    break;
                }
                found_count += 1;
            }

            if found_count == count {
                // Found it!
                let mut chain = RunList::new();
                chain.push(rimio::run::Run {
                    start: range_start as u64,
                    length: count as u64,
                });
                driver.write_run_list(io, &chain)?;

                self.free_count = self.free_count.saturating_sub(count);
                self.next_free_hint = range_start + count as u32;
                if self.next_free_hint > end {
                    self.next_free_hint = wrap_limit;
                }

                return Ok(FatHandle::from(chain));
            }

            // Advance
            current += 1;
            if current > end {
                current = wrap_limit;
            }
            searched_count += 1;
        }

        Err(FsAllocatorError::OutOfBlocks)
    }

    fn used_units(&self) -> usize {
        (self.meta.cluster_count as usize).saturating_sub(self.free_count)
    }

    fn remaining_units(&self) -> usize {
        self.free_count
    }
}
