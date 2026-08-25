// SPDX-License-Identifier: MIT

use crate::prelude::*;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

//
// Run and RunList
//

/// Represents a contiguous run of units (blocks, clusters, sectors) on the storage media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Run {
    /// Starting offset (usually in units like clusters or blocks)
    pub start: u64,
    /// Number of contiguous units
    pub length: u64,
}

impl Run {
    pub const fn new(start: u64, length: u64) -> Self {
        Self { start, length }
    }
}

/// A list of physical runs, typically representing a fragmented allocation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunList(pub Vec<Run>);

impl RunList {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, run: Run) {
        if let Some(last) = self.0.last_mut()
            && last.start + last.length == run.start
        {
            last.length += run.length;
            return;
        }
        self.0.push(run);
    }

    /// Appends a single unit to the run list, merging it if possible.
    pub fn push_unit(&mut self, unit: u64) {
        self.push(Run::new(unit, 1));
    }

    /// Creates a run list containing a single unit.
    pub fn from_unit(unit: u64) -> Self {
        let mut list = Self::new();
        list.push_unit(unit);
        list
    }

    /// Calculate total number of units in the list
    pub fn total_units(&self) -> u64 {
        self.0.iter().map(|r| r.length).sum()
    }

    pub fn from_units(units: &[u32]) -> Self {
        let mut list = Self::new();
        if units.is_empty() {
            return list;
        }

        let mut current_start = units[0] as u64;
        let mut current_len = 1;

        for &unit in &units[1..] {
            if unit as u64 == current_start + current_len {
                current_len += 1;
            } else {
                list.push(Run::new(current_start, current_len));
                current_start = unit as u64;
                current_len = 1;
            }
        }
        list.push(Run::new(current_start, current_len));
        list
    }

    pub fn to_units(&self) -> Vec<u32> {
        let mut units = Vec::new();
        for run in &self.0 {
            for i in 0..run.length {
                units.push((run.start + i) as u32);
            }
        }
        units
    }

    pub fn iter(&self) -> core::slice::Iter<'_, Run> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Gets the physical unit ID at the given logical index within this run list.
    pub fn get_unit(&self, index: u64) -> Option<u64> {
        let mut current_idx = 0;
        for run in &self.0 {
            if index < current_idx + run.length {
                return Some(run.start + (index - current_idx));
            }
            current_idx += run.length;
        }
        None
    }

    /// Extends this run list by appending another.
    pub fn extend(&mut self, other: &Self) {
        for run in other.iter() {
            self.push(*run);
        }
    }

    /// Checks if the entire run list represents a single contiguous block of units.
    pub fn is_contiguous(&self) -> bool {
        self.0.len() <= 1
    }
}

//
// MappedRimIO
//

/// A wrapper that implements `RimIO` over a fragmented `RunList`.
pub struct MappedRimIO<'a, IO: RimIO + ?Sized> {
    inner: &'a mut IO,
    runs: &'a RunList,
    unit_size: usize,
    /// Offset of the first unit of the run at `last_run_idx`
    last_run_base: u64,
    /// Index of the last accessed run in `runs`
    last_run_idx: usize,
}

impl<'a, IO: RimIO + ?Sized> MappedRimIO<'a, IO> {
    pub fn new(inner: &'a mut IO, runs: &'a RunList, unit_size: usize) -> Self {
        Self {
            inner,
            runs,
            unit_size,
            last_run_base: 0,
            last_run_idx: 0,
        }
    }

    #[inline]
    fn find_run(&mut self, logical_unit_idx: u64) -> RimIOResult<(&Run, u64, usize)> {
        // Optimization: start searching from the last used run index
        // This makes sequential access O(1) in the common case.
        let mut current_unit_base = if logical_unit_idx >= self.last_run_base {
            self.last_run_base
        } else {
            0
        };
        let start_idx = if logical_unit_idx >= self.last_run_base {
            self.last_run_idx
        } else {
            0
        };

        for (idx, run) in self.runs.iter().enumerate().skip(start_idx) {
            if logical_unit_idx < current_unit_base + run.length {
                self.last_run_base = current_unit_base;
                self.last_run_idx = idx;
                return Ok((run, current_unit_base, idx));
            }
            current_unit_base += run.length;
        }

        // If not found in the skip-search, try a full search if we hadn't already
        if start_idx > 0 {
            let mut current_unit_base = 0;
            for (idx, run) in self.runs.iter().enumerate() {
                if logical_unit_idx < current_unit_base + run.length {
                    self.last_run_base = current_unit_base;
                    self.last_run_idx = idx;
                    return Ok((run, current_unit_base, idx));
                }
                current_unit_base += run.length;
                if idx >= start_idx {
                    break;
                }
            }
        }

        Err(RimIOError::OutOfBounds)
    }
}

impl<'a, IO: RimIO + ?Sized> RimIO for MappedRimIO<'a, IO> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let mut remaining = buf.len();
        let mut logical_ptr = offset;
        let mut buf_ptr = 0;
        let us = self.unit_size as u64;

        while remaining > 0 {
            let logical_unit_idx = logical_ptr / us;
            let offset_in_unit = logical_ptr % us;

            let (run, base, _) = self.find_run(logical_unit_idx)?;

            let physical_unit = run.start + (logical_unit_idx - base);
            let physical_byte_offset = (physical_unit * us) + offset_in_unit;

            let units_remaining_in_run = run.length - (logical_unit_idx - base);
            let bytes_remaining_in_run = (units_remaining_in_run * us) - offset_in_unit;

            let to_process = remaining.min(bytes_remaining_in_run as usize);

            self.inner.read_at(
                physical_byte_offset,
                &mut buf[buf_ptr..buf_ptr + to_process],
            )?;

            remaining -= to_process;
            logical_ptr += to_process as u64;
            buf_ptr += to_process;
        }

        Ok(())
    }

    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let mut remaining = data.len();
        let mut logical_ptr = offset;
        let mut data_ptr = 0;
        let us = self.unit_size as u64;

        while remaining > 0 {
            let logical_unit_idx = logical_ptr / us;
            let offset_in_unit = logical_ptr % us;

            let (run, base, _) = self.find_run(logical_unit_idx)?;

            let physical_unit = run.start + (logical_unit_idx - base);
            let physical_byte_offset = (physical_unit * us) + offset_in_unit;

            let units_remaining_in_run = run.length - (logical_unit_idx - base);
            let bytes_remaining_in_run = (units_remaining_in_run * us) - offset_in_unit;

            let to_process = remaining.min(bytes_remaining_in_run as usize);

            self.inner
                .write_at(physical_byte_offset, &data[data_ptr..data_ptr + to_process])?;

            remaining -= to_process;
            logical_ptr += to_process as u64;
            data_ptr += to_process;
        }

        Ok(())
    }

    fn flush(&mut self) -> RimIOResult {
        self.inner.flush()
    }

    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.inner.set_offset(partition_offset)
    }

    fn partition_offset(&self) -> u64 {
        self.inner.partition_offset()
    }
}

//
// MappedRun and MappedRunList
//

/// Represents a run mapped to a specific logical offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MappedRun {
    /// Logical offset (in units)
    pub logical_offset: u64,
    /// Physical run on disk
    pub physical: Run,
}

impl MappedRun {
    pub const fn new(logical_offset: u64, physical: Run) -> Self {
        Self {
            logical_offset,
            physical,
        }
    }
}

/// A list of mapped runs, providing a complete logical-to-physical mapping.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MappedRunList(pub Vec<MappedRun>);

impl MappedRunList {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, run: MappedRun) {
        if let Some(last) = self.0.last_mut() {
            let last_phys_end = last.physical.start + last.physical.length;
            let last_logical_end = last.logical_offset + last.physical.length;

            if last_phys_end == run.physical.start && last_logical_end == run.logical_offset {
                last.physical.length += run.physical.length;
                return;
            }
        }
        self.0.push(run);
    }

    pub fn from_run_list(runs: &RunList, logical_start: u64) -> Self {
        let mut list = Self::new();
        let mut current_offset = logical_start;

        for &run in runs.iter() {
            list.push(MappedRun::new(current_offset, run));
            current_offset += run.length;
        }
        list
    }

    pub fn iter(&self) -> core::slice::Iter<'_, MappedRun> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
