// SPDX-License-Identifier: MIT

//! Core filesystem metadata contracts and geometry definitions.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::string::String;

pub use crate::utils::volume::*;

/// Trait implemented by each FS-specific Meta structure.
/// Provides access to static metadata needed during formatting, allocation, injection, or checking.
pub trait FsMeta<Unit: Ord + Copy> {
    /// Size of one allocation unit in bytes.
    fn unit_size(&self) -> u64;

    /// Compute the offset (in bytes) on disk corresponding to a given allocation unit.
    fn unit_offset(&self, unit: Unit) -> u64;

    /// Root unit (root cluster/inode).
    fn root_unit(&self) -> Unit;

    /// First valid unit for allocation.
    fn first_data_unit(&self) -> Unit;

    /// Last valid unit.
    fn last_data_unit(&self) -> Unit;

    /// Total number of allocatable units.
    fn total_units(&self) -> u64;

    /// Total size in bytes of the FS.
    fn size_bytes(&self) -> u64;

    /// Volume label.
    fn label(&self) -> String;

    /// Check if a given unit is valid for this FS.
    fn is_valid_unit(&self, unit: Unit) -> bool {
        unit >= self.first_data_unit() && unit <= self.last_data_unit()
    }
}
