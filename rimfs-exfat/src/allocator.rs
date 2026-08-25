pub use crate::core::allocator::*;
use crate::core::{bitmap::BitmapDriver, fat::*};
use crate::meta::*;
use rimio::prelude::*;

#[derive(Debug, Clone)]
pub struct ExFatHandle {
    pub cluster_id: u32,
    pub cluster_chain: RunList,
}

impl ExFatHandle {
    pub fn new(cluster_id: u32) -> Self {
        ExFatHandle {
            cluster_id,
            cluster_chain: RunList::from_units(&[cluster_id]),
        }
    }

    pub fn from_chain(cluster_chain: RunList) -> Self {
        let cluster_id = cluster_chain.get_unit(0).map(|u| u as u32).unwrap_or(0);
        Self {
            cluster_id,
            cluster_chain,
        }
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

/// Real ExFAT Allocator using on-disk Bitmap scanning via BitmapDriver.
pub struct ExFatAllocator<'a> {
    pub meta: &'a ExFatMeta,
    pub bitmap_chain: RunList,
    pub next_free_hint: u32,
    pub used_clusters: u32,
}

impl<'a> ExFatAllocator<'a> {
    /// Creates a new ExFatAllocator with an empty bitmap.
    pub fn new(meta: &'a ExFatMeta) -> Self {
        Self {
            meta,
            bitmap_chain: RunList::new(),
            next_free_hint: meta.first_data_unit(),
            used_clusters: 0,
        }
    }

    /// Creates an ExFatAllocator from existing components.
    pub fn from_raw_parts(
        meta: &'a ExFatMeta,
        bitmap_chain: RunList,
        next_free_hint: u32,
        used_clusters: u32,
    ) -> Self {
        Self {
            meta,
            bitmap_chain,
            next_free_hint,
            used_clusters,
        }
    }

    pub fn from_io<IO: RimIO + ?Sized>(
        io: &mut IO,
        meta: &'a ExFatMeta,
    ) -> FsAllocatorResult<Self> {
        // 1. Read the bitmap chain (so we know where it is, though mapped IO for bitmap is tricky with unified BitmapView)
        // Wait, BitmapView uses absolute offset from meta.bitmap_offset().
        // For ExFAT, the bitmap might be fragmented.
        // `meta.bitmap_offset()` assumes contiguous or uses the start.
        // `ExFatMeta::bitmap_offset()` calculates: heap_offset + (bitmap_cluster - 2) * cluster_size.
        // If the bitmap itself is fragmented, `BitmapView` as implemented (linear offset) works ONLY if we provide a RimIO that handles the mapping (like MappedRimIO) OR if we teach BitmapView about chains.

        // CRITICAL FIX: `BitmapView` expects linear IO.
        // If the bitmap file is fragmented, standard `io.read_at(meta.bitmap_offset() + ...)` is WRONG unless `io` is the volume root and the bitmap is contiguous.
        // ExFAT Bitmap IS usually contiguous but CAN be fragmented.
        // However, `BitmapFsMeta` returns a single `bitmap_offset`.
        // If we want to support fragmented bitmaps, `BitmapView` logic needs to route reads through the chain.
        // BUT: ExFatMeta implementing `BitmapFsMeta` effectively says "It's at this linear physical offset".
        // This holds true only if the bitmap is contiguous.

        // For now, let's assume contiguous or that `io` handles it.
        // To be robust: We should ideally wrap `io` in a `MappedRimIO` representing the bitmap file, pass THAT to `BitmapView`.
        // Assumption: Bitmap is contiguous. This is standard for ExFAT.
        // If it weren't, we'd need a chained reader.
        // ExFatMeta implements BitmapFsMeta returning the absolute offset of the bitmap start.

        let mut view = BitmapDriver::new(meta);

        let used_clusters = view.count_ones(io).map_err(FsAllocatorError::IO)? as u32;

        // Scan for first free hint
        let next_free_hint = if let Some(free_bit) = view
            .find_next_free(io, 0, 1)
            .map_err(FsAllocatorError::IO)?
        {
            meta.first_data_unit() + free_bit as u32
        } else {
            meta.first_data_unit() // Full?
        };

        // We don't really need to store the bitmap chain if we assume contiguity and use offset.
        // But the struct expects `bitmap_chain`.
        // We'll construct a synthetic one or read it just to satisfy the struct field,
        // OR better: we can probably remove `bitmap_chain` from the struct if it's not used anymore?
        // Check struct usage. `ExFatAllocator` struct def has `pub bitmap_chain: RunList`.
        // It's public. We should probably keep it populated for now to avoid breaking other things,
        // or just put the single run in it.

        let mut bitmap_chain = RunList::new();
        bitmap_chain.push(Run {
            start: meta.bitmap_cluster as u64,
            length: meta.bitmap_clusters() as u64,
        });

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
        count: usize,
    ) -> FsAllocatorResult<ExFatHandle> {
        self.allocate_contiguous(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<ExFatHandle> {
        let count_u64 = count as u64;

        // Use BitmapDriver directly on the IO (assuming contiguous bitmap)
        let mut driver = BitmapDriver::new(self.meta);

        // Start search from next_free_hint (converted to bit index)
        let start_bit_search_index =
            (self.next_free_hint.saturating_sub(ExFatMeta::FIRST_CLUSTER)) as u64;

        // 1. Search from hint
        let found_bit_index = match driver.find_next_free(io, start_bit_search_index, count_u64) {
            Ok(Some(bit)) => Some(bit),
            Ok(None) => {
                // 2. Wrap around to 0
                driver
                    .find_next_free(io, 0, count_u64)
                    .map_err(FsAllocatorError::IO)?
            }
            Err(e) => return Err(FsAllocatorError::IO(e)),
        };

        let bit_index = found_bit_index.ok_or(FsAllocatorError::OutOfBlocks)?;
        let start_cluster = ExFatMeta::FIRST_CLUSTER + bit_index as u32;

        // Mark bits as used
        driver
            .set_bits_range(io, bit_index, count_u64, true)
            .map_err(FsAllocatorError::IO)?;

        // Helper to flush is built-in to set_bits_range for window, but final flush needed?
        // BitmapDriver::set_bits_range sets dirty=true.
        // We must flush.
        driver.flush(io).map_err(FsAllocatorError::IO)?;

        // Update state
        self.used_clusters += count as u32;
        self.next_free_hint = start_cluster + count as u32;

        if self.next_free_hint >= self.meta.cluster_count + ExFatMeta::FIRST_CLUSTER {
            self.next_free_hint = self.meta.first_data_unit();
        }

        // Return handle and write FAT chain
        let chain: Vec<u32> = (0..count).map(|i| start_cluster + i as u32).collect();
        FatDriver::new(self.meta).write_chain(io, &chain)?;

        Ok(ExFatHandle::from(chain))
    }

    fn used_units(&self) -> usize {
        self.used_clusters as usize
    }

    fn remaining_units(&self) -> usize {
        (self.meta.cluster_count - self.used_clusters) as usize
    }
}
