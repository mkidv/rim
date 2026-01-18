// SPDX-License-Identifier: MIT
pub mod chain_allocator;

pub use crate::core::errors::{FsAllocatorError, FsAllocatorResult};

/// Trait implemented by all FS allocation handles.
///
/// Example: cluster handle, inode handle, block handle, etc.
pub trait FsHandle {}

/// Trait for managing allocation of logical units in a filesystem.
///
/// - `Handle` is a handle representing an allocated unit (e.g., containing metadata or chains)
pub trait FsAllocator<Handle: FsHandle + Sized + Clone> {
    /// Allocate `count` units and return a handle per unit.
    #[must_use = "allocation result must be checked for errors"]
    fn allocate_chain(&mut self, count: usize) -> FsAllocatorResult<Handle>;

    /// Allocate a single unit and return its handle.
    #[must_use = "allocation result must be checked for errors"]
    fn allocate_unit(&mut self) -> FsAllocatorResult<Handle> {
        self.allocate_chain(1)
    }

    /// Number of units currently used.
    fn used_units(&self) -> usize;

    /// Number of remaining units.
    fn remaining_units(&self) -> usize;
}
