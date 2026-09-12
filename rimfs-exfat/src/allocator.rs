// SPDX-License-Identifier: MIT

//! exFAT cluster allocator and allocation bitmap synchronization.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

pub use crate::core::allocator::*;

use crate::core::{bitmap::BitmapDriver, fat::*};
use crate::meta::*;

use rimio::prelude::*;

/// Handle for an ExFAT allocation.
#[derive(Debug, Clone)]
pub struct ExFatHandle {
    pub cluster_id: u32,
    pub cluster_chain: RunList,
}

impl ExFatHandle {
    /// Create a handle containing a single cluster.
    pub fn new(cluster_id: u32) -> Self {
        Self {
            cluster_id,
            cluster_chain: RunList::from_unit(cluster_id as u64),
        }
    }

    /// Create a handle from an existing cluster chain.
    pub fn from_chain(cluster_chain: RunList) -> Self {
        let cluster_id = cluster_chain
            .get_unit(0)
            .map(|unit| unit as u32)
            .unwrap_or(0);

        Self {
            cluster_id,
            cluster_chain,
        }
    }

    /// Number of clusters represented by this handle.
    #[inline]
    pub fn cluster_count(&self) -> u64 {
        self.cluster_chain.total_units()
    }
}

impl FsHandle for ExFatHandle {}

impl From<RunList> for ExFatHandle {
    fn from(chain: RunList) -> Self {
        Self::from_chain(chain)
    }
}

impl From<Vec<u32>> for ExFatHandle {
    fn from(chain: Vec<u32>) -> Self {
        Self::from_chain(RunList::from_units(&chain))
    }
}

/// Real ExFAT allocator backed by the on-disk Allocation Bitmap.
///
/// `next_free_hint` is expressed as an ExFAT cluster number.
///
/// Bitmap indices use a different coordinate system:
///
/// ```text
/// bitmap bit 0 -> cluster FIRST_CLUSTER
/// bitmap bit n -> cluster FIRST_CLUSTER + n
/// ```
pub struct ExFatAllocator<'a> {
    pub meta: &'a ExFatMeta,

    /// Physical cluster chain occupied by the Allocation Bitmap.
    pub bitmap_chain: RunList,

    /// Cluster number from which the next allocation search should start.
    pub next_free_hint: u32,

    /// Number of currently allocated clusters.
    pub used_clusters: u64,
}

impl<'a> ExFatAllocator<'a> {
    /// Creates an allocator for a freshly-created filesystem.
    ///
    /// The initial hint skips the statically-positioned system structures.
    pub fn new(meta: &'a ExFatMeta) -> Self {
        let mut bitmap_chain = RunList::new();
        bitmap_chain.push(Run::new(
            meta.bitmap_cluster as u64,
            meta.bitmap_clusters() as u64,
        ));

        Self {
            meta,
            bitmap_chain,
            next_free_hint: meta.first_data_unit(),
            used_clusters: 0,
        }
    }

    /// Creates an allocator from explicit state.
    pub fn from_raw_parts(
        meta: &'a ExFatMeta,
        bitmap_chain: RunList,
        next_free_hint: u32,
        used_clusters: u64,
    ) -> Self {
        Self {
            meta,
            bitmap_chain,
            next_free_hint,
            used_clusters,
        }
    }

    /// Reconstruct allocator state from an existing ExFAT Allocation Bitmap.
    pub fn from_io<R: RimRead + ?Sized>(
        io: &mut R,
        meta: &'a ExFatMeta,
    ) -> FsAllocatorResult<Self> {
        let mut driver = BitmapDriver::new(meta);

        let used_clusters = driver.count_ones_ro(io).map_err(FsAllocatorError::IO)?;

        // Bitmap bit 0 corresponds to FIRST_CLUSTER, not first_data_unit().
        let next_free_hint = match driver
            .find_next_free_ro(io, 0, 1)
            .map_err(FsAllocatorError::IO)?
        {
            Some(bit) => {
                let bit = u32::try_from(bit).map_err(|_| FsAllocatorError::OutOfBlocks)?;

                ExFatMeta::FIRST_CLUSTER
                    .checked_add(bit)
                    .ok_or(FsAllocatorError::OutOfBlocks)?
            }
            None => ExFatMeta::FIRST_CLUSTER,
        };

        let mut bitmap_chain = RunList::new();
        bitmap_chain.push(Run::new(
            meta.bitmap_cluster as u64,
            meta.bitmap_clusters() as u64,
        ));

        Ok(Self::from_raw_parts(
            meta,
            bitmap_chain,
            next_free_hint,
            used_clusters,
        ))
    }
}

impl<'a> FsAllocator<ExFatHandle> for ExFatAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<ExFatHandle> {
        if count == 0 {
            return Err(FsAllocatorError::InvalidSize);
        }

        // Prefer one contiguous allocation.
        match self.allocate_contiguous(io, count) {
            Ok(handle) => return Ok(handle),
            Err(FsAllocatorError::OutOfBlocks) => {}
            Err(error) => return Err(error),
        }

        // Fall back to fragmented free space.
        let mut driver = BitmapDriver::new(self.meta);

        // Returned runs are expressed in bitmap-bit coordinates.
        let bit_runs = driver
            .find_free_runs(io, count)
            .map_err(FsAllocatorError::IO)?
            .ok_or(FsAllocatorError::OutOfBlocks)?;

        // Reserve all selected bitmap runs.
        driver
            .set_run_list(io, &bit_runs, true)
            .map_err(FsAllocatorError::IO)?;

        driver.flush(io).map_err(FsAllocatorError::IO)?;

        // Translate bitmap coordinates into ExFAT cluster coordinates.
        let mut cluster_chain = RunList::new();

        for run in bit_runs.iter() {
            let start = run
                .start
                .checked_add(ExFatMeta::FIRST_CLUSTER as u64)
                .ok_or(FsAllocatorError::OutOfBlocks)?;

            cluster_chain.push(Run::new(start, run.length));
        }

        // Materialize the cluster chain in the FAT.
        //
        // Bitmap is committed first intentionally: an interrupted operation may
        // leak clusters, but must not leave clusters reusable while referenced
        // by a FAT chain.
        FatDriver::new(self.meta)
            .write_run_list(io, &cluster_chain)
            .map_err(FsAllocatorError::IO)?;

        self.used_clusters = self
            .used_clusters
            .saturating_add(cluster_chain.total_units());

        Ok(ExFatHandle::from_chain(cluster_chain))
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<ExFatHandle> {
        if count == 0 {
            return Err(FsAllocatorError::InvalidSize);
        }

        let mut driver = BitmapDriver::new(self.meta);

        // Convert ExFAT cluster coordinate -> bitmap bit coordinate.
        let start_bit = u64::from(self.next_free_hint.saturating_sub(ExFatMeta::FIRST_CLUSTER));

        // Search from the hint, then wrap once to the beginning.
        let bit_index = match driver
            .find_next_free(io, start_bit, count)
            .map_err(FsAllocatorError::IO)?
        {
            Some(bit) => bit,

            None if start_bit != 0 => driver
                .find_next_free(io, 0, count)
                .map_err(FsAllocatorError::IO)?
                .ok_or(FsAllocatorError::OutOfBlocks)?,

            None => return Err(FsAllocatorError::OutOfBlocks),
        };

        // Defensive check. BitmapDriver should already enforce
        // bitmap_valid_bits(), but keeping this invariant explicit is cheap.
        let end_bit = bit_index
            .checked_add(count)
            .ok_or(FsAllocatorError::OutOfBlocks)?;

        if end_bit > self.meta.total_units() {
            return Err(FsAllocatorError::OutOfBlocks);
        }

        // Reserve the bitmap range.
        driver
            .set_bits_range(io, bit_index, count, true)
            .map_err(FsAllocatorError::IO)?;

        driver.flush(io).map_err(FsAllocatorError::IO)?;

        // Convert bitmap bit -> ExFAT cluster number.
        let bit_index_u32 = u32::try_from(bit_index).map_err(|_| FsAllocatorError::OutOfBlocks)?;

        let start_cluster = ExFatMeta::FIRST_CLUSTER
            .checked_add(bit_index_u32)
            .ok_or(FsAllocatorError::OutOfBlocks)?;

        // Represent the contiguous allocation as a single Run.
        let mut cluster_chain = RunList::new();
        cluster_chain.push(Run::new(start_cluster as u64, count));

        // Materialize the chain in the FAT.
        FatDriver::new(self.meta)
            .write_run_list(io, &cluster_chain)
            .map_err(FsAllocatorError::IO)?;

        self.used_clusters = self.used_clusters.saturating_add(count);

        // Update the hint in bitmap space first, then convert back to
        // an ExFAT cluster coordinate.
        self.next_free_hint = if end_bit >= self.meta.total_units() {
            ExFatMeta::FIRST_CLUSTER
        } else {
            let next_bit = u32::try_from(end_bit).map_err(|_| FsAllocatorError::OutOfBlocks)?;

            ExFatMeta::FIRST_CLUSTER
                .checked_add(next_bit)
                .ok_or(FsAllocatorError::OutOfBlocks)?
        };

        Ok(ExFatHandle::from_chain(cluster_chain))
    }

    fn used_units(&self) -> u64 {
        self.used_clusters
    }

    fn remaining_units(&self) -> u64 {
        self.meta.total_units().saturating_sub(self.used_clusters)
    }
}
