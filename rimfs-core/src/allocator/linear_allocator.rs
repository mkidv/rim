// SPDX-License-Identifier: MIT

//! Linear contiguous block and sector allocator.

use rimio::RimIO;

use crate::allocator::{FsAllocator, FsAllocatorError, FsAllocatorResult, FsHandle};

/// A high-performance, zero-allocation linear bump allocator.
///
/// Maintains a cursor and sequentially allocates contiguous ranges of units
/// until reaching `max_units`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinearAllocator {
    pub current: u64,
    pub max_units: u64,
    pub base_offset: u64,
}

impl LinearAllocator {
    /// Create a new linear allocator starting at `start` and bounded by `max_units`.
    pub const fn new(start: u64, max_units: u64) -> Self {
        Self {
            current: start,
            max_units,
            base_offset: start,
        }
    }

    /// Allocate a contiguous range of `count` units, returning the starting unit number.
    pub fn allocate_range(&mut self, count: u64) -> FsAllocatorResult<u64> {
        if count == 0 {
            return Err(FsAllocatorError::InvalidSize);
        }

        let start = self.current;
        let end = start
            .checked_add(count)
            .ok_or(FsAllocatorError::OutOfBlocks)?;
        if end > self.max_units {
            return Err(FsAllocatorError::OutOfBlocks);
        }

        self.current = end;
        Ok(start)
    }

    /// Reset cursor back to the base starting unit.
    pub fn reset(&mut self) {
        self.current = self.base_offset;
    }

    /// Number of units allocated so far.
    #[inline]
    pub fn used_units(&self) -> u64 {
        self.current.saturating_sub(self.base_offset)
    }

    /// Number of remaining allocatable units.
    #[inline]
    pub fn remaining_units(&self) -> u64 {
        self.max_units.saturating_sub(self.current)
    }
}

impl<H> FsAllocator<H> for LinearAllocator
where
    H: FsHandle + From<u64>,
{
    fn allocate<IO: RimIO + ?Sized>(&mut self, _io: &mut IO, count: u64) -> FsAllocatorResult<H> {
        self.allocate_range(count).map(H::from)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<H> {
        self.allocate_range(count).map(H::from)
    }

    fn used_units(&self) -> u64 {
        self.used_units()
    }

    fn remaining_units(&self) -> u64 {
        self.remaining_units()
    }
}
