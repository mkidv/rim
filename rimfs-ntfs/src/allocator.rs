// SPDX-License-Identifier: MIT
//! NTFS cluster allocator

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::RimIO;
use rimio::run::{Run, RunList};

pub use crate::core::allocator::*;

use crate::core::meta::FsMeta;
use crate::meta::NtfsMeta;

use crate::core::bitmap::BitmapDriver;

/// Handle for an NTFS allocation
///
/// Unlike FAT-based filesystems, NTFS uses data runs (extents) for
/// non-contiguous allocation. This handle tracks the allocated clusters.
#[derive(Debug, Clone)]
pub struct NtfsHandle {
    /// Starting LCN (Logical Cluster Number)
    pub start_lcn: u64,
    /// Run list of allocated clusters (data runs)
    pub runs: RunList,
}

impl NtfsHandle {
    /// Create a new handle starting at the given LCN
    pub fn new(start_lcn: u64) -> Self {
        Self {
            start_lcn,
            runs: RunList::from_unit(start_lcn),
        }
    }

    /// Create a handle from a contiguous range
    pub fn from_range(start_lcn: u64, count: u64) -> Self {
        let mut runs = RunList::new();
        runs.push(Run::new(start_lcn, count));
        Self { start_lcn, runs }
    }

    /// Create a handle from a list of clusters
    pub fn from_clusters(clusters: Vec<u64>) -> Self {
        let mut runs = RunList::new();
        // Convert Vec<u64> to RunList. RunList::from_units takes &[u32] but we have u64.
        for &c in &clusters {
            runs.push_unit(c);
        }
        let start_lcn = clusters.first().copied().unwrap_or(0);
        Self { start_lcn, runs }
    }

    /// Create a handle from an existing RunList
    pub fn from_run_list(runs: RunList) -> Self {
        let start_lcn = runs.0.first().map(|r| r.start).unwrap_or(0);
        Self { start_lcn, runs }
    }

    /// Get the total number of allocated clusters
    pub fn cluster_count(&self) -> u64 {
        self.runs.total_units()
    }
}

impl FsHandle for NtfsHandle {}

/// NTFS cluster allocator
///
/// Manages the cluster bitmap for allocation tracking.
/// Uses a buffered bitmap driver to interact with the on-disk $Bitmap.
pub struct NtfsAllocator<'a> {
    pub meta: &'a NtfsMeta,
    pub next_free_hint: u64,
    pub used_clusters: u64,
    pub driver: BitmapDriver<'a, NtfsMeta>,
}

impl<'a> NtfsAllocator<'a> {
    /// Create a new allocator from metadata with default scan.
    pub fn new(meta: &'a NtfsMeta) -> FsAllocatorResult<Self> {
        Ok(Self {
            meta,
            next_free_hint: meta.first_data_unit(),
            used_clusters: 0,
            driver: BitmapDriver::new(meta),
        })
    }

    /// Read the bitmap from disk to initialize state.
    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO, meta: &'a NtfsMeta) -> FsAllocatorResult<Self> {
        let mut driver = BitmapDriver::new(meta);
        let used = driver.count_ones(io).map_err(FsAllocatorError::IO)? as u64;

        // Scan for first free hint starting from data area
        let hint = driver
            .find_next_free(io, meta.first_data_unit(), 1)
            .map_err(FsAllocatorError::IO)?
            .unwrap_or(meta.first_data_unit());

        Ok(Self {
            meta,
            next_free_hint: hint,
            used_clusters: used,
            driver,
        })
    }

    /// Flush any dirty bitmap window cached in memory to disk.
    pub fn flush<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> FsAllocatorResult<()> {
        self.driver.flush(io).map_err(FsAllocatorError::IO)
    }

    /// Free a contiguous range of clusters back to the allocator.
    pub fn free_range<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        start: u64,
        count: u64,
    ) -> FsAllocatorResult<()> {
        if count == 0 {
            return Ok(());
        }
        self.driver
            .set_bits_range(io, start, count, false)
            .map_err(FsAllocatorError::IO)?;
        self.driver.flush(io).map_err(FsAllocatorError::IO)?;
        self.used_clusters = self.used_clusters.saturating_sub(count);
        if start < self.next_free_hint {
            self.next_free_hint = start;
        }
        Ok(())
    }
}

// Note: FsAllocator trait requires allocate to take `&mut self` and `io`.
// BUT FsAllocator trait signature for allocator types:
// impl<B: BitmapOps> FsAllocator<BitmapHandle> for BitmapAllocator<B>
//
// Our NtfsAllocator now uses BitmapDriver which requires IO for most ops.
// FsAllocator's MAIN method `allocate` takes IO.
// So we can implement FsAllocator.
impl<'a> FsAllocator<NtfsHandle> for NtfsAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<NtfsHandle> {
        self.allocate_contiguous(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<NtfsHandle> {
        let count_u64 = count as u64;
        let start_search = self.next_free_hint;

        let found = match self.driver.find_next_free(io, start_search, count_u64) {
            Ok(Some(start)) => Some(start),
            _ => {
                if start_search > 0 {
                    self.driver.find_next_free(io, 0, count_u64).ok().flatten()
                } else {
                    None
                }
            }
        };

        if let Some(start) = found {
            self.driver
                .set_bits_range(io, start, count_u64, true)
                .map_err(FsAllocatorError::IO)?;
            self.driver.flush(io).map_err(FsAllocatorError::IO)?;

            self.used_clusters += count_u64;
            self.next_free_hint = start + count_u64;
            if self.next_free_hint >= self.meta.total_clusters {
                self.next_free_hint = self.meta.first_data_unit();
            }
            return Ok(NtfsHandle::from_range(start, count_u64));
        }

        Err(FsAllocatorError::OutOfBlocks)
    }

    fn used_units(&self) -> usize {
        self.used_clusters as usize
    }

    fn remaining_units(&self) -> usize {
        (self.meta.total_clusters - self.used_clusters) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimio::prelude::MemRimIO;

    #[test]
    fn test_allocator_basic() {
        let meta = NtfsMeta::new(100 * 1024 * 1024, Some("TEST")).unwrap();
        let mut buffer = vec![0u8; 4 * 1024 * 1024]; // 4MB buffer to accommodate system area
        let mut io = MemRimIO::new(&mut buffer);

        // We need to initialize the bitmap area on disk
        let mut driver = BitmapDriver::new(&meta);
        driver.format_with(&mut io, 0).unwrap();

        let mut alloc = NtfsAllocator::new(&meta).unwrap();

        let first_free = meta.first_data_unit();

        // Mark system area used
        driver.set_bits_range(&mut io, 0, first_free, true).unwrap();
        driver.flush(&mut io).unwrap();

        // Clusters before first_free should be marked used
        for i in 0..first_free {
            assert!(
                driver.get_bit(&mut io, i).unwrap(),
                "Cluster {} should be reserved by system",
                i
            );
        }
        assert!(!driver.get_bit(&mut io, first_free).unwrap());

        // Allocate some clusters
        let handle = alloc.allocate_contiguous(&mut io, 10).unwrap();
        assert_eq!(handle.cluster_count(), 10);
        assert_eq!(handle.start_lcn, first_free);

        // Invalidate test driver cache to see the changes made by allocator
        driver.valid = false;

        // Those clusters should now be used

        // Those clusters should now be used
        for lcn in handle.start_lcn..handle.start_lcn + 10 {
            assert!(driver.get_bit(&mut io, lcn).unwrap());
        }
    }
}
