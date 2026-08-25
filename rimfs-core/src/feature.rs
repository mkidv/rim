// SPDX-License-Identifier: MIT

use crate::errors::FsFeatureResult;

/// Trait representing a distinct system feature/component of a filesystem.
///
/// Examples include: Journal, Allocation Bitmaps, MFT, Superblock backups.
///
/// This trait decouples the "Formatted" state from the individual components,
/// allowing them to be initialized, allocated, and written independently.
pub trait FsSystemFeature<M, A, IO: ?Sized> {
    /// Returns the human-readable name of the feature (e.g. "Journal", "MFT").
    fn name(&self) -> &str;

    /// Step 1: Calculate requirements based on metadata.
    ///
    /// Updates internal state with necessary size/alignment/location constraints.
    /// Does NOT perform allocation or IO.
    fn prepare(&mut self, meta: &M) -> FsFeatureResult<()>;

    /// Step 2: Allocate storage for the feature.
    ///
    /// Uses the provided allocator to reserve space on the disk.
    /// Returns the handle to the allocated space.
    fn allocate(&mut self, io: &mut IO, allocator: &mut A) -> FsFeatureResult<()>;

    /// Step 3: Write the initial content of the feature to disk.
    ///
    /// The feature should use its internal handle (set during `allocate`) to know where to write.
    /// The allocator is provided to access any dynamic allocation state needed for writing (e.g. bitmaps).
    fn write(&self, io: &mut IO, allocator: &A) -> FsFeatureResult<()>;

    /// Step 4 (Optional): Read/Verify the feature from disk.
    ///
    /// Used during filesystem checking (`fsck`) or mounting.
    fn read(&mut self, io: &mut IO) -> FsFeatureResult<()> {
        let _ = io;
        Ok(())
    }
}
