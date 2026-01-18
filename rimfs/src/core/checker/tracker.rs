// SPDX-License-Identifier: MIT

//! Reachability tracker for filesystem tree walks.
//!
//! Provides a memory-efficient structure for tracking which units (clusters, blocks, inodes)
//! have been visited during a filesystem tree traversal.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::core::utils::bitmap::BitmapOps;

/// Tracks unit reachability/visitation during FS tree walks.
///
/// Memory-efficient: uses 1 bit per unit instead of 1 byte (like `Vec<bool>`).
/// Handles the base unit offset internally, so callers can use raw unit numbers.
///
/// # Example
/// ```ignore
/// use rimfs::core::checker::ReachabilityTracker;
///
/// // Create tracker for clusters 2..1000002 (1M clusters starting at cluster 2)
/// let mut tracker = ReachabilityTracker::new(2, 1_000_000);
///
/// // Mark cluster 5 as reachable
/// tracker.mark(5);
/// assert!(tracker.is_marked(5));
///
/// // Mark a contiguous range
/// tracker.mark_range(100, 50); // clusters 100..150
/// ```
#[derive(Debug, Clone)]
pub struct ReachabilityTracker {
    bitmap: Vec<u8>,
    base_unit: u32,
    count: usize,
}

impl ReachabilityTracker {
    /// Creates a tracker for `count` units starting at `base_unit`.
    ///
    /// # Arguments
    /// * `base_unit` - The first valid unit number (e.g., `FIRST_CLUSTER` for FAT/ExFAT, `1` for EXT4 inodes)
    /// * `count` - Total number of units to track
    pub fn new(base_unit: u32, count: usize) -> Self {
        Self {
            bitmap: vec![0u8; count.div_ceil(8)],
            base_unit,
            count,
        }
    }

    /// Marks a unit as reachable/visited.
    ///
    /// Does nothing if the unit is out of range.
    #[inline]
    pub fn mark(&mut self, unit: u32) {
        if unit < self.base_unit {
            return;
        }
        let idx = (unit - self.base_unit) as usize;
        if idx < self.count {
            self.bitmap.set_bit(idx, true);
        }
    }

    /// Marks a contiguous range `[start, start+len)` as reachable.
    ///
    /// Useful for marking cluster runs from `ClusterCursor::for_each_run`.
    #[inline]
    pub fn mark_range(&mut self, start: u32, len: u32) {
        if start < self.base_unit {
            return;
        }
        let start_idx = (start - self.base_unit) as usize;
        for i in 0..len as usize {
            let idx = start_idx + i;
            if idx < self.count {
                self.bitmap.set_bit(idx, true);
            }
        }
    }

    /// Checks if a unit is marked as reachable.
    #[inline]
    pub fn is_marked(&self, unit: u32) -> bool {
        if unit < self.base_unit {
            return false;
        }
        let idx = (unit - self.base_unit) as usize;
        if idx < self.count {
            self.bitmap.get_bit(idx)
        } else {
            false
        }
    }

    /// Returns the raw bitmap for comparison with on-disk bitmaps.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bitmap
    }

    /// Returns the number of tracked units.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Returns the base unit offset.
    pub fn base_unit(&self) -> u32 {
        self.base_unit
    }

    /// Counts orphan units: positions where `on_disk[i]=1` but `self[i]=0`.
    ///
    /// # Arguments
    /// * `on_disk` - The on-disk allocation bitmap to compare against
    ///
    /// # Returns
    /// Number of orphan units (allocated on disk but not reachable)
    pub fn count_orphans(&self, on_disk: &[u8]) -> usize {
        let mut orphans = 0usize;
        let min_len = self.bitmap.len().min(on_disk.len());

        for (disk_byte, reach_byte) in on_disk.iter().zip(self.bitmap.iter()).take(min_len) {
            // Orphan = on disk but not reachable
            let orphan_bits = disk_byte & !reach_byte;
            orphans += orphan_bits.count_ones() as usize;
        }

        // Mask out trailing bits beyond self.count
        let valid_bits_in_last = self.count % 8;
        if valid_bits_in_last != 0 && min_len > 0 {
            let last_idx = min_len - 1;
            // We already counted all 8 bits, need to subtract invalid ones
            let disk_byte = on_disk[last_idx];
            let reach_byte = self.bitmap[last_idx];
            let orphan_bits = disk_byte & !reach_byte;

            // Count bits that are beyond valid range
            let invalid_mask = !((1u8 << valid_bits_in_last) - 1);
            let invalid_orphans = (orphan_bits & invalid_mask).count_ones() as usize;
            orphans = orphans.saturating_sub(invalid_orphans);
        }

        orphans
    }

    /// Iterates over orphan units, calling the callback for each.
    ///
    /// # Arguments
    /// * `on_disk` - The on-disk allocation bitmap
    /// * `limit` - Maximum number of orphans to report (for sampling)
    /// * `f` - Callback receiving the unit number of each orphan
    pub fn for_each_orphan<F>(&self, on_disk: &[u8], limit: usize, mut f: F)
    where
        F: FnMut(u32),
    {
        let mut found = 0usize;
        let min_len = self.bitmap.len().min(on_disk.len());

        for (i, (disk_byte, reach_byte)) in on_disk
            .iter()
            .zip(self.bitmap.iter())
            .enumerate()
            .take(min_len)
        {
            if found >= limit {
                break;
            }

            let orphan_bits = disk_byte & !reach_byte;

            if orphan_bits != 0 {
                for bit in 0..8 {
                    let idx = i * 8 + bit;
                    if idx >= self.count {
                        break;
                    }
                    if (orphan_bits & (1 << bit)) != 0 {
                        f(self.base_unit + idx as u32);
                        found += 1;
                        if found >= limit {
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_mark() {
        let mut tracker = ReachabilityTracker::new(2, 100);

        // Mark cluster 5
        tracker.mark(5);
        assert!(tracker.is_marked(5));
        assert!(!tracker.is_marked(4));
        assert!(!tracker.is_marked(6));
    }

    #[test]
    fn test_mark_range() {
        let mut tracker = ReachabilityTracker::new(2, 100);

        // Mark clusters 10..15
        tracker.mark_range(10, 5);

        for c in 10..15 {
            assert!(tracker.is_marked(c), "cluster {c} should be marked");
        }
        assert!(!tracker.is_marked(9));
        assert!(!tracker.is_marked(15));
    }

    #[test]
    fn test_out_of_range() {
        let mut tracker = ReachabilityTracker::new(2, 10);

        // Marking beyond range should not panic
        tracker.mark(100);
        assert!(!tracker.is_marked(100));

        // Below base also shouldn't panic
        tracker.mark(0);
        // This will map to negative index, saturating to 0, which IS valid
        // Actually: 0 - 2 = saturating gives large number, won't be < 10
        assert!(!tracker.is_marked(0));
    }

    #[test]
    fn test_count_orphans() {
        let mut tracker = ReachabilityTracker::new(0, 16);

        // Mark bits 0, 1, 2, 3 as reachable
        tracker.mark_range(0, 4);

        // On-disk has bits 0, 1, 2, 3, 4, 5 set
        let on_disk = [0b00111111u8, 0b00000000];

        // Orphans are 4, 5 (on disk but not reachable)
        assert_eq!(tracker.count_orphans(&on_disk), 2);
    }

    #[test]
    fn test_for_each_orphan() {
        let mut tracker = ReachabilityTracker::new(2, 16);

        // Mark clusters 2, 3, 4 as reachable
        tracker.mark_range(2, 3);

        // On-disk bitmap (relative to cluster 2) has clusters 2,3,4,5,6 set
        let on_disk = [0b00011111u8, 0b00000000];

        let mut orphans = Vec::new();
        tracker.for_each_orphan(&on_disk, 10, |unit| orphans.push(unit));

        // Orphans should be clusters 5, 6 (base_unit + 3, base_unit + 4)
        assert_eq!(orphans, vec![5, 6]);
    }
}
