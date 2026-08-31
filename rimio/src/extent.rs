// SPDX-License-Identifier: MIT
//! Extent mapping abstraction for lazy random-access I/O.
//!
//! Translates logical file offsets into source storage offsets over an ordered
//! list of extents (including contiguous runs, fragmented blocks, and sparse holes).

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

#[cfg(feature = "alloc")]
use crate::{RimIOError, RimIOResult, RimRead};

/// Represents a contiguous mapping between logical offsets and backing source offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoExtent {
    /// Logical byte offset within the file/stream.
    pub logical_offset: u64,
    /// Offset on the backing source I/O device.
    /// If `None`, this represents a sparse hole (reads return zeros).
    pub source_offset: Option<u64>,
    /// Length of this extent in bytes.
    pub len: u64,
}

impl IoExtent {
    /// Creates a non-sparse extent mapping.
    #[inline]
    pub const fn new(logical_offset: u64, source_offset: u64, len: u64) -> Self {
        Self {
            logical_offset,
            source_offset: Some(source_offset),
            len,
        }
    }

    /// Creates a sparse hole extent (reads return zeroes without backing I/O).
    #[inline]
    pub const fn hole(logical_offset: u64, len: u64) -> Self {
        Self {
            logical_offset,
            source_offset: None,
            len,
        }
    }

    /// Returns the logical end offset (exclusive).
    #[inline]
    pub fn logical_end(&self) -> u64 {
        self.logical_offset.saturating_add(self.len)
    }
}

/// Lazy random-access reader over an ordered list of [`IoExtent`] mappings.
///
/// Implements [`RimRead`] on top of any backing [`RimRead`] source.
/// Translates logical file offsets to source offsets without buffering the file into memory.
#[cfg(feature = "alloc")]
pub struct ExtentRimRead<S> {
    source: S,
    extents: Vec<IoExtent>,
    total_size: u64,
    last_extent_idx: usize,
}

#[cfg(feature = "alloc")]
impl<S> ExtentRimRead<S> {
    /// Creates a new `ExtentRimRead` for a single contiguous extent.
    pub fn from_contiguous(source: S, source_offset: u64, total_size: u64) -> Self {
        let mut extents = Vec::with_capacity(1);
        if total_size > 0 {
            extents.push(IoExtent::new(0, source_offset, total_size));
        }
        Self {
            source,
            extents,
            total_size,
            last_extent_idx: 0,
        }
    }

    /// Creates a new `ExtentRimRead` from a list of extents, automatically coalescing adjacent runs.
    pub fn new(source: S, raw_extents: Vec<IoExtent>, total_size: u64) -> Self {
        let mut extents: Vec<IoExtent> = Vec::with_capacity(raw_extents.len());
        for ext in raw_extents {
            if ext.len == 0 {
                continue;
            }
            if let Some(last) = extents.last_mut() {
                let last_end = last.logical_offset.saturating_add(last.len);
                if last_end == ext.logical_offset {
                    match (last.source_offset, ext.source_offset) {
                        (Some(p1), Some(p2)) if p1.saturating_add(last.len) == p2 => {
                            last.len = last.len.saturating_add(ext.len);
                            continue;
                        }
                        (None, None) => {
                            last.len = last.len.saturating_add(ext.len);
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            extents.push(ext);
        }

        Self {
            source,
            extents,
            total_size,
            last_extent_idx: 0,
        }
    }

    /// Returns the total logical size in bytes.
    #[inline]
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    /// Returns a reference to the backing source.
    #[inline]
    pub fn source(&self) -> &S {
        &self.source
    }

    /// Returns a mutable reference to the backing source.
    #[inline]
    pub fn source_mut(&mut self) -> &mut S {
        &mut self.source
    }

    /// Consumes the wrapper, returning the inner backing source.
    #[inline]
    pub fn into_inner(self) -> S {
        self.source
    }

    /// Returns the extent slice.
    #[inline]
    pub fn extents(&self) -> &[IoExtent] {
        &self.extents
    }

    /// Finds the extent index containing the given logical offset.
    #[inline]
    fn find_extent_idx(&mut self, logical_offset: u64) -> Option<usize> {
        if self.extents.is_empty() {
            return None;
        }

        // Fast path: check last used extent
        if self.last_extent_idx < self.extents.len() {
            let ext = &self.extents[self.last_extent_idx];
            if logical_offset >= ext.logical_offset && logical_offset < ext.logical_end() {
                return Some(self.last_extent_idx);
            }
            // Check next sequential extent
            if self.last_extent_idx + 1 < self.extents.len() {
                let next = &self.extents[self.last_extent_idx + 1];
                if logical_offset >= next.logical_offset && logical_offset < next.logical_end() {
                    self.last_extent_idx += 1;
                    return Some(self.last_extent_idx);
                }
            }
        }

        // Binary search fallback
        match self.extents.binary_search_by(|ext| {
            if logical_offset < ext.logical_offset {
                core::cmp::Ordering::Greater
            } else if logical_offset >= ext.logical_end() {
                core::cmp::Ordering::Less
            } else {
                core::cmp::Ordering::Equal
            }
        }) {
            Ok(idx) => {
                self.last_extent_idx = idx;
                Some(idx)
            }
            Err(_) => None,
        }
    }
}

#[cfg(feature = "alloc")]
impl<S: RimRead> RimRead for ExtentRimRead<S> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        if buf.is_empty() {
            return Ok(());
        }

        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(RimIOError::OutOfBounds)?;

        if end > self.total_size {
            return Err(RimIOError::OutOfBounds);
        }

        // Fast path: single contiguous extent covering the entire file
        if self.extents.len() == 1 {
            let ext = &self.extents[0];
            match ext.source_offset {
                Some(source_base) => {
                    let source_pos = source_base
                        .checked_add(offset.saturating_sub(ext.logical_offset))
                        .ok_or(RimIOError::OutOfBounds)?;
                    return self.source.read_at(source_pos, buf);
                }
                None => {
                    buf.fill(0);
                    return Ok(());
                }
            }
        }

        let mut remaining = buf;
        let mut cur_logical = offset;

        while !remaining.is_empty() {
            let idx = self
                .find_extent_idx(cur_logical)
                .ok_or(RimIOError::OutOfBounds)?;
            let ext = self.extents[idx];

            let offset_in_ext = cur_logical - ext.logical_offset;
            let bytes_in_ext = ext.len.saturating_sub(offset_in_ext);
            let to_read = (remaining.len() as u64).min(bytes_in_ext) as usize;

            if to_read == 0 {
                return Err(RimIOError::OutOfBounds);
            }

            match ext.source_offset {
                Some(source_base) => {
                    let source_pos = source_base
                        .checked_add(offset_in_ext)
                        .ok_or(RimIOError::OutOfBounds)?;
                    self.source.read_at(source_pos, &mut remaining[..to_read])?;
                }
                None => {
                    // Sparse hole: zero fill
                    remaining[..to_read].fill(0);
                }
            }

            cur_logical = cur_logical.saturating_add(to_read as u64);
            remaining = &mut remaining[to_read..];
        }

        Ok(())
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.total_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SliceRimIO;

    #[test]
    fn test_extent_contiguous() {
        let backing = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut slice_io = SliceRimIO::new(backing);

        // Map logical 0..10 to source 10..20 ("ABCDEFGHIJ")
        let mut extent_reader = ExtentRimRead::from_contiguous(&mut slice_io, 10, 10);
        assert_eq!(extent_reader.total_size(), 10);

        let mut buf = [0u8; 10];
        extent_reader.read_at(0, &mut buf).unwrap();
        assert_eq!(&buf, b"ABCDEFGHIJ");

        let mut small_buf = [0u8; 4];
        extent_reader.read_at(2, &mut small_buf).unwrap();
        assert_eq!(&small_buf, b"CDEF");

        // Out of bounds
        assert!(extent_reader.read_at(8, &mut small_buf).is_err());
    }

    #[test]
    fn test_extent_multi_fragmented_and_holes() {
        let backing = b"AAAABBBBCCCCDDDDEEEE";
        let mut slice_io = SliceRimIO::new(backing);

        // Construct fragmented file:
        // [0..4]   -> source 8..12 ("CCCC")
        // [4..8]   -> Hole (4 bytes zeroes)
        // [8..12]  -> source 0..4 ("AAAA")
        // [12..16] -> source 16..20 ("EEEE")
        let extents = alloc::vec![
            IoExtent::new(0, 8, 4),
            IoExtent::hole(4, 4),
            IoExtent::new(8, 0, 4),
            IoExtent::new(12, 16, 4),
        ];

        let mut reader = ExtentRimRead::new(&mut slice_io, extents, 16);
        assert_eq!(reader.total_size(), 16);

        let mut full_buf = [0u8; 16];
        reader.read_at(0, &mut full_buf).unwrap();
        assert_eq!(&full_buf, b"CCCC\0\0\0\0AAAAEEEE");

        // Read crossing extent boundaries (spanning hole)
        let mut cross_buf = [0u8; 8];
        reader.read_at(2, &mut cross_buf).unwrap();
        assert_eq!(&cross_buf, b"CC\0\0\0\0AA");
    }

    #[test]
    fn test_extent_coalescing() {
        let raw = alloc::vec![
            IoExtent::new(0, 100, 10),
            IoExtent::new(10, 110, 20), // Should coalesce into (0, 100, 30)
            IoExtent::hole(30, 5),
            IoExtent::hole(35, 10), // Should coalesce into hole(30, 15)
            IoExtent::new(45, 500, 5),
        ];

        let reader = ExtentRimRead::new(SliceRimIO::new(b""), raw, 50);
        let ext = reader.extents();
        assert_eq!(ext.len(), 3);
        assert_eq!(ext[0], IoExtent::new(0, 100, 30));
        assert_eq!(ext[1], IoExtent::hole(30, 15));
        assert_eq!(ext[2], IoExtent::new(45, 500, 5));
    }
}
