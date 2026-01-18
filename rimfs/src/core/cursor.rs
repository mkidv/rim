// SPDX-License-Identifier: MIT

use crate::FsMeta;
use crate::core::errors::FsCursorError;
use crate::core::{FsCursorResult, fat};
use rimio::prelude::*;

/// Trait for cluster-based filesystems (FAT32, ExFAT, etc.)
pub trait ClusterMeta: FsMeta<u32> {
    const EOC: u32;
    const FIRST_CLUSTER: u32;
    const ENTRY_SIZE: usize;
    const ENTRY_MASK: u32; // Mask to isolate useful bits

    /// Computes the offset of a FAT entry
    fn fat_entry_offset(&self, cluster: u32, fat_index: u8) -> u64;

    /// Checks if a cluster is End-of-Chain
    fn is_eoc(&self, cluster: u32) -> bool {
        cluster >= Self::EOC
    }

    fn num_fats(&self) -> u8;
}

/// Generic cursor for cluster-based filesystems.
///
/// This cursor abstracts cluster chain traversal logic and automatically
/// optimizes I/O by grouping contiguous runs.
///
/// # Type Parameters
/// - `M`: Type implementing `FsMeta<u32>` (Fat32Meta, ExFatMeta, etc.)
/// - `C`: FS-specific constants (EOC, FIRST_CLUSTER, etc.)
#[derive(Debug)]
pub struct ClusterCursor<'a, M>
where
    M: ClusterMeta,
{
    meta: &'a M,
    current: Option<u32>,
    seen: usize,
    allow_system: bool,
}

impl<'a, M> ClusterCursor<'a, M>
where
    M: ClusterMeta,
{
    pub fn new_safe(meta: &'a M, start: u32) -> Self {
        Self {
            meta,
            current: Some(start),
            seen: 0,
            allow_system: false,
        }
    }

    pub fn new(meta: &'a M, start: u32) -> Self {
        Self {
            meta,
            current: Some(start),
            seen: 0,
            allow_system: true,
        }
    }

    #[inline]
    fn in_bounds(&self, c: u32) -> bool {
        let min = if self.allow_system {
            M::FIRST_CLUSTER
        } else {
            self.meta.first_data_unit()
        };
        let max = self.meta.last_data_unit();
        (min..=max).contains(&c)
    }

    /// One iteration step (uses integrated read_fat_entry)
    pub fn next_with<IO>(&mut self, io: &mut IO) -> Option<FsCursorResult<u32>>
    where
        IO: RimIO + ?Sized,
    {
        let c = self.current?;
        self.seen += 1;
        if self.seen > self.meta.total_units() {
            self.current = None;
            return Some(Err(FsCursorError::LoopDetected));
        }

        let next = match fat::chain::read_entry(io, self.meta, c, 0) {
            Ok(n) => n,
            Err(e) => {
                self.current = None;
                return Some(Err(e.into()));
            }
        };

        // Check end-of-chain & bounds
        if !self.in_bounds(c) {
            self.current = None;
            return Some(Err(FsCursorError::InvalidCluster(c)));
        }
        if self.meta.is_eoc(next) {
            self.current = None;
        } else {
            if !self.in_bounds(next) {
                self.current = None;
                return Some(Err(FsCursorError::InvalidCluster(next)));
            }
            self.current = Some(next);
        }
        Some(Ok(c))
    }

    /// Iterate cluster by cluster via callback
    pub fn for_each_cluster<IO, F>(&mut self, io: &mut IO, mut f: F) -> FsCursorResult<()>
    where
        IO: RimIO + ?Sized,
        F: FnMut(&mut IO, u32) -> FsCursorResult<()>,
    {
        while let Some(res) = self.next_with(io) {
            let c = res?;
            f(io, c)?;
        }
        Ok(())
    }

    /// Iterate by contiguous runs (start, len) via callback
    pub fn for_each_run<IO, F>(&mut self, io: &mut IO, mut f: F) -> FsCursorResult<()>
    where
        IO: RimIO + ?Sized,
        F: FnMut(&mut IO, u32, u32) -> FsCursorResult<()>,
    {
        let mut start: Option<u32> = None;
        let mut prev: Option<u32> = None;
        let mut len: u32 = 0;

        let mut flush = |io: &mut IO, s: Option<u32>, l: u32| -> FsCursorResult<()> {
            if let Some(s0) = s
                && l > 0
            {
                f(io, s0, l)?;
            }
            Ok(())
        };

        while let Some(res) = self.next_with(io) {
            let c = res?;
            match prev {
                Some(p) if c == p + 1 => {
                    len += 1;
                }
                Some(_) => {
                    flush(io, start, len)?;
                    start = Some(c);
                    len = 1;
                }
                None => {
                    start = Some(c);
                    len = 1;
                }
            }
            prev = Some(c);
        }
        flush(io, start, len)?;
        Ok(())
    }

    /// Creates a cluster-by-cluster iterator
    pub fn iter<'b, IO>(&'b mut self, io: &'b mut IO) -> ClusterIter<'a, 'b, M, IO>
    where
        IO: RimIO + ?Sized,
    {
        ClusterIter { cursor: self, io }
    }

    /// Creates an iterator over contiguous **runs**
    pub fn runs<'b, IO>(&'b mut self, io: &'b mut IO) -> RunIter<'a, 'b, M, IO>
    where
        IO: RimIO + ?Sized,
    {
        RunIter {
            cursor: self,
            io,
            start: None,
            prev: None,
            len: 0,
            finished: false,
        }
    }
}

/// Cluster-by-cluster iterator
pub struct ClusterIter<'a, 'b, M, IO: ?Sized>
where
    M: ClusterMeta,
{
    cursor: &'b mut ClusterCursor<'a, M>,
    io: &'b mut IO,
}

impl<'a, 'b, M, IO: ?Sized> Iterator for ClusterIter<'a, 'b, M, IO>
where
    M: ClusterMeta,
    IO: RimIO,
{
    type Item = FsCursorResult<u32>;

    fn next(&mut self) -> Option<Self::Item> {
        self.cursor.next_with(self.io)
    }
}

/// Contiguous runs iterator
pub struct RunIter<'a, 'b, M, IO: ?Sized>
where
    M: ClusterMeta,
{
    cursor: &'b mut ClusterCursor<'a, M>,
    io: &'b mut IO,
    start: Option<u32>,
    prev: Option<u32>,
    len: u32,
    finished: bool,
}

impl<'a, 'b, M, IO: ?Sized> Iterator for RunIter<'a, 'b, M, IO>
where
    M: ClusterMeta,
    IO: RimIO,
{
    type Item = FsCursorResult<(u32, u32)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        loop {
            match self.cursor.next_with(self.io) {
                Some(Ok(c)) => {
                    match self.prev {
                        Some(p) if c == p + 1 => {
                            self.len += 1;
                            self.prev = Some(c);
                        }
                        Some(_) => {
                            // Run break → return the previous one
                            let out = (self.start.unwrap(), self.len);
                            // initialize the new run with c
                            self.start = Some(c);
                            self.prev = Some(c);
                            self.len = 1;
                            return Some(Ok(out));
                        }
                        None => {
                            self.start = Some(c);
                            self.prev = Some(c);
                            self.len = 1;
                        }
                    }
                }
                Some(Err(e)) => {
                    self.finished = true;
                    return Some(Err(e));
                }
                None => {
                    // End: if there's an incomplete run left, return it one last time
                    self.finished = true;
                    if let (Some(s), l @ 1..) = (self.start, self.len) {
                        return Some(Ok((s, l)));
                    }
                    return None;
                }
            }
        }
    }
}

/// Linear (contiguous) cursor: traverses range [start .. start+clusters).
/// Does NOT consult the FAT. Ideal for NOFATCHAIN entries (Upcase, Bitmap, etc.).
#[derive(Clone, Copy, Debug)]
pub struct LinearCursor<'a, M: ClusterMeta> {
    meta: &'a M,
    next: u32,
    end_excl: u32,
    allow_system: bool,
}

impl<'a, M: ClusterMeta> Iterator for LinearCursor<'a, M> {
    type Item = FsCursorResult<u32>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end_excl {
            return None;
        }
        let c = self.next;
        self.next = self.next.saturating_add(1);
        if !self.in_bounds(c) {
            return Some(Err(FsCursorError::InvalidCluster(c)));
        }
        Some(Ok(c))
    }
}

impl<'a, M: ClusterMeta> LinearCursor<'a, M> {
    /// Constructs from a cluster count
    #[inline]
    pub fn from_clusters_safe(meta: &'a M, start: u32, clusters: u32) -> Self {
        Self {
            meta,
            next: start,
            end_excl: start.saturating_add(clusters),
            allow_system: false,
        }
    }

    /// Constructs from a logical length in bytes
    #[inline]
    pub fn from_len_bytes_safe(meta: &'a M, start: u32, len_bytes: u64) -> Self {
        let cs = meta.unit_size() as u64;
        let clusters = len_bytes.div_ceil(cs); // ceil
        Self::from_clusters_safe(meta, start, clusters as u32)
    }

    #[inline]
    pub fn from_clusters(meta: &'a M, start: u32, clusters: u32) -> Self {
        Self {
            meta,
            next: start,
            end_excl: start.saturating_add(clusters),
            allow_system: true,
        }
    }

    /// Constructs from a logical length in bytes
    #[inline]
    pub fn from_len_bytes(meta: &'a M, start: u32, len_bytes: u64) -> Self {
        let cs = meta.unit_size() as u64;
        let clusters = len_bytes.div_ceil(cs); // ceil
        Self::from_clusters(meta, start, clusters as u32)
    }

    #[inline]
    fn in_bounds(&self, c: u32) -> bool {
        let min = if self.allow_system {
            M::FIRST_CLUSTER
        } else {
            self.meta.first_data_unit()
        };
        let max = self.meta.last_data_unit();
        (min..=max).contains(&c)
    }

    /// Iterates by **contiguous runs** (start, len) and calls `f(io, start, len)`.
    /// `IO` is passed to allow direct I/O batching in the callback.
    pub fn for_each_run<IO, F>(&mut self, io: &mut IO, mut f: F) -> FsCursorResult<()>
    where
        IO: RimIO + ?Sized,
        F: FnMut(&mut IO, u32, u32) -> FsCursorResult<()>,
    {
        // Since it's linear, the entire range is a single run if size > 0.
        if self.next < self.end_excl {
            let start = self.next;
            if !self.in_bounds(start) {
                return Err(FsCursorError::InvalidCluster(start));
            }
            let len = self.end_excl - self.next;
            // minimal validation: upper bound
            let last = start.saturating_add(len - 1);

            if !self.in_bounds(last) {
                return Err(FsCursorError::InvalidCluster(last));
            }

            // Consume everything at once
            self.next = self.end_excl;
            f(io, start, len)?;
        }
        Ok(())
    }

    /// Reads stream directly into `dst`, batching by runs.
    /// `total_len` = logical bytes to read (truncates last run if needed).
    pub fn read_into<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        total_len: usize,
        dst: &mut [u8],
    ) -> FsCursorResult<()> {
        assert!(dst.len() >= total_len);
        let cs = self.meta.unit_size();
        let mut written = 0usize;

        self.for_each_run(io, |io, run_start, run_len| {
            if written >= total_len {
                return Ok(());
            }
            // Size in bytes of this run
            let run_bytes = (run_len as usize) * cs;
            let to_copy = core::cmp::min(run_bytes, total_len - written);
            if to_copy > 0 {
                let off = self.meta.unit_offset(run_start);
                io.read_at(off, &mut dst[written..written + to_copy])?;
                written += to_copy;
            }
            Ok(())
        })?;

        if written < total_len {
            return Err(FsCursorError::Other("linear_short_read"));
        }
        Ok(())
    }
}
