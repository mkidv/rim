// SPDX-License-Identifier: MIT
#![allow(dead_code)]

use crate::{BlockIO, BlockIOResult};

/// Simple counters, no_std friendly.
#[derive(Clone, Copy, Default, Debug)]
pub struct IoStats {
    pub reads: u64,
    pub read_bytes: u64,
    pub writes: u64,
    pub write_bytes: u64,
    pub flushes: u64,

    // Alignment (useful to observe the effect of write_block_best_effort)
    pub aligned_reads: u64,
    pub unaligned_reads: u64,
    pub aligned_writes: u64,
    pub unaligned_writes: u64,

    // Useful sizes to diagnose granularity
    pub max_read: u64,
    pub max_write: u64,
}

impl IoStats {
    #[inline] pub fn reset(&mut self) { *self = IoStats::default(); }
}

/// Transparent instrumentation wrapper.
pub struct IOCounter<'a, IO: BlockIO + ?Sized> {
    inner: &'a mut IO,
    pub stats: IoStats,
    /// Optional: local "block" alignment (e.g., 512, 4096, cluster_size...)
    pub align: u64,
}

impl<'a, IO: BlockIO + ?Sized> IOCounter<'a, IO> {
    #[inline]
    pub fn new(inner: &'a mut IO) -> Self {
        Self { inner, stats: IoStats::default(), align: 1 }
    }

    #[inline]
    pub fn with_align(inner: &'a mut IO, align: u64) -> Self {
        let align = if align == 0 { 1 } else { align };
        Self { inner, stats: IoStats::default(), align }
    }

    #[inline] pub fn snapshot(&self) -> IoStats { self.stats }
    #[inline] pub fn into_inner(self) -> &'a mut IO { self.inner }
}

impl<'a, IO: BlockIO + ?Sized> BlockIO for IOCounter<'a, IO> {
    #[inline]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> BlockIOResult {
        let aligned = (offset % self.align == 0) && (data.len() as u64 % self.align == 0);
        if aligned { self.stats.aligned_writes += 1; } else { self.stats.unaligned_writes += 1; }

        self.stats.writes += 1;
        self.stats.write_bytes += data.len() as u64;
        if self.stats.max_write < data.len() as u64 { self.stats.max_write = data.len() as u64; }

        self.inner.write_at(offset, data)
    }

    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> BlockIOResult {
        let aligned = (offset % self.align == 0) && (buf.len() as u64 % self.align == 0);
        if aligned { self.stats.aligned_reads += 1; } else { self.stats.unaligned_reads += 1; }

        self.stats.reads += 1;
        self.stats.read_bytes += buf.len() as u64;
        if self.stats.max_read < buf.len() as u64 { self.stats.max_read = buf.len() as u64; }

        self.inner.read_at(offset, buf)
    }

    #[inline]
    fn flush(&mut self) -> BlockIOResult {
        self.stats.flushes += 1;
        self.inner.flush()
    }

    #[inline] fn set_offset(&mut self, p: u64) -> u64 { self.inner.set_offset(p) }
    #[inline] fn partition_offset(&self) -> u64 { self.inner.partition_offset() }
}

pub trait IOTracer {
    fn on_read(&mut self, _off: u64, _len: usize) {}
    fn on_write(&mut self, _off: u64, _len: usize) {}
    fn on_flush(&mut self) {}
}

pub struct TracingIO<'a, IO: BlockIO + ?Sized, Tr: IOTracer> {
    inner: &'a mut IO,
    tracer: Tr,
}

impl<'a, IO: BlockIO + ?Sized, Tr: IOTracer> BlockIO for TracingIO<'a, IO, Tr> {
    fn write_at(&mut self, off: u64, data: &[u8]) -> BlockIOResult {
        self.tracer.on_write(off, data.len());
        self.inner.write_at(off, data)
    }
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> BlockIOResult {
        self.tracer.on_read(off, buf.len());
        self.inner.read_at(off, buf)
    }
    fn flush(&mut self) -> BlockIOResult {
        self.tracer.on_flush();
        self.inner.flush()
    }
    fn set_offset(&mut self, p: u64) -> u64 { self.inner.set_offset(p) }
    fn partition_offset(&self) -> u64 { self.inner.partition_offset() }
}
