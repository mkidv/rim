// SPDX-License-Identifier: MIT

//! Common block and cluster allocator interfaces.

pub mod linear_allocator;

pub use crate::errors::{FsAllocatorError, FsAllocatorResult};
use rimio::RimIO;

/// Trait implemented by all FS allocation handles.
///
/// Example: cluster handle, inode handle, block handle, etc.
pub trait FsHandle {}

impl FsHandle for () {}

/// Trait for managing allocation of logical units in a filesystem.
///
/// `Handle` is a handle representing an allocated unit (e.g., containing metadata or chains)
pub trait FsAllocator<Handle: FsHandle> {
    /// Allocate `count` logical units.
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<Handle>;

    /// Allocate `count` contiguous logical units.
    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<Handle>;

    /// Allocate a single logical unit.
    #[must_use = "allocation result must be checked for errors"]
    fn allocate_unit<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> FsAllocatorResult<Handle> {
        self.allocate(io, 1)
    }

    /// Number of logical units currently used.
    fn used_units(&self) -> u64;

    /// Number of logical units remaining.
    fn remaining_units(&self) -> u64;
}
