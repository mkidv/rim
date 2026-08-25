// SPDX-License-Identifier: MIT
pub mod linear_allocator;

pub use crate::errors::{FsAllocatorError, FsAllocatorResult};
use rimio::RimIO;

/// Trait implemented by all FS allocation handles.
///
/// Example: cluster handle, inode handle, block handle, etc.
pub trait FsHandle {}

/// Trait for managing allocation of logical units in a filesystem.
///
/// - `Handle` is a handle representing an allocated unit (e.g., containing metadata or chains)
pub trait FsAllocator<Handle: FsHandle + Sized + Clone> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<Handle>;

    /// Allocate a contiguous range of units and return its handle.
    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<Handle>;

    /// Allocate a single unit and return its handle.
    #[must_use = "allocation result must be checked for errors"]
    fn allocate_unit<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> FsAllocatorResult<Handle> {
        self.allocate(io, 1)
    }

    /// Number of units currently used.
    fn used_units(&self) -> usize;

    /// Number of remaining units.
    fn remaining_units(&self) -> usize;
}
