// SPDX-License-Identifier: MIT
#![allow(dead_code)]

use crate::{RimIO, RimIOResult};

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
    #[inline]
    pub fn reset(&mut self) {
        *self = IoStats::default();
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.reads == 0 && self.writes == 0 && self.flushes == 0
    }
    #[inline]
    pub fn avg_read(&self) -> u64 {
        if self.reads == 0 {
            0
        } else {
            self.read_bytes / self.reads
        }
    }
    #[inline]
    pub fn avg_write(&self) -> u64 {
        if self.writes == 0 {
            0
        } else {
            self.write_bytes / self.writes
        }
    }
    #[inline]
    pub fn read_aligned_ratio(&self) -> (u64, u64) {
        let tot = self.aligned_reads + self.unaligned_reads;
        (self.aligned_reads, tot)
    }
    #[inline]
    pub fn write_aligned_ratio(&self) -> (u64, u64) {
        let tot = self.aligned_writes + self.unaligned_writes;
        (self.aligned_writes, tot)
    }
    #[inline]
    pub fn merge(&mut self, other: &IoStats) {
        self.reads += other.reads;
        self.read_bytes += other.read_bytes;
        self.writes += other.writes;
        self.write_bytes += other.write_bytes;
        self.flushes += other.flushes;
        self.aligned_reads += other.aligned_reads;
        self.unaligned_reads += other.unaligned_reads;
        self.aligned_writes += other.aligned_writes;
        self.unaligned_writes += other.unaligned_writes;
        if other.max_read > self.max_read {
            self.max_read = other.max_read;
        }
        if other.max_write > self.max_write {
            self.max_write = other.max_write;
        }
    }
    #[inline]
    pub fn delta(&self, before: &IoStats) -> IoStats {
        IoStats {
            reads: self.reads.saturating_sub(before.reads),
            read_bytes: self.read_bytes.saturating_sub(before.read_bytes),
            writes: self.writes.saturating_sub(before.writes),
            write_bytes: self.write_bytes.saturating_sub(before.write_bytes),
            flushes: self.flushes.saturating_sub(before.flushes),
            aligned_reads: self.aligned_reads.saturating_sub(before.aligned_reads),
            unaligned_reads: self.unaligned_reads.saturating_sub(before.unaligned_reads),
            aligned_writes: self.aligned_writes.saturating_sub(before.aligned_writes),
            unaligned_writes: self
                .unaligned_writes
                .saturating_sub(before.unaligned_writes),
            max_read: self.max_read.saturating_sub(before.max_read), // maxes are less meaningful as deltas; keep simple
            max_write: self.max_write.saturating_sub(before.max_write),
        }
    }
}

// ---- Byte pretty-printer (no_std) ----
#[inline]
fn fmt_bytes(n: u64, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut idx = 0;
    let mut whole = n;
    let mut frac = 0u64;

    while whole >= 1024 && idx < UNITS.len() - 1 {
        frac = whole % 1024;
        whole /= 1024;
        idx += 1;
    }

    // Show one decimal when it helps (e.g. 1.5 KiB)
    if idx > 0 && frac != 0 {
        // scale fraction to one decimal: (frac / 1024) * 10 ≈ (frac * 10) / 1024
        let tenths = (frac * 10 + 512) / 1024; // rounded
        if tenths > 0 && tenths < 10 {
            return write!(f, "{}.{} {}", whole, tenths, UNITS[idx]);
        }
    }
    write!(f, "{} {}", whole, UNITS[idx])
}

// ---- Percent helper ----
#[inline]
fn fmt_pct(numer: u64, denom: u64, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    if denom == 0 {
        return write!(f, "—");
    }
    let pct = (numer as f64) * 100.0 / (denom as f64);
    // avoid pulling in formatting heavy machinery; keep simple
    // show with 0 or 1 decimal depending on size
    if pct.fract() == 0.0 {
        write!(f, "{}%", pct as u64)
    } else {
        write!(f, "{pct:.1}%")
    }
}

// ---- Display implementation ----
impl core::fmt::Display for IoStats {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (r_align, r_tot) = self.read_aligned_ratio();
        let (w_align, w_tot) = self.write_aligned_ratio();

        if !f.alternate() {
            // Compact one-liner
            write!(f, "R: {}×, ", self.reads)?;
            fmt_bytes(self.read_bytes, f)?;
            write!(f, " (")?;
            fmt_pct(r_align, r_tot, f)?;
            write!(f, " aligned, avg ")?;
            fmt_bytes(self.avg_read(), f)?;
            write!(f, ") | W: {}×, ", self.writes)?;
            fmt_bytes(self.write_bytes, f)?;
            write!(f, " (")?;
            fmt_pct(w_align, w_tot, f)?;
            write!(f, " aligned, avg ")?;
            fmt_bytes(self.avg_write(), f)?;
            write!(f, ") | F: {}× | Max r=", self.flushes)?;
            fmt_bytes(self.max_read, f)?;
            write!(f, ", w=")?;
            fmt_bytes(self.max_write, f)
        } else {
            // Pretty multi-line
            writeln!(f, "Reads   : {:>6} ops | total ", self.reads)?;
            fmt_bytes(self.read_bytes, f)?;
            write!(f, " | avg ")?;
            fmt_bytes(self.avg_read(), f)?;
            write!(f, " | aligned ")?;
            fmt_pct(r_align, r_tot, f)?;
            writeln!(f)?;

            writeln!(f, "Writes  : {:>6} ops | total ", self.writes)?;
            fmt_bytes(self.write_bytes, f)?;
            write!(f, " | avg ")?;
            fmt_bytes(self.avg_write(), f)?;
            write!(f, " | aligned ")?;
            fmt_pct(w_align, w_tot, f)?;
            writeln!(f)?;

            writeln!(f, "Flushes : {:>6} ops", self.flushes)?;
            write!(f, "Max     : read ")?;
            fmt_bytes(self.max_read, f)?;
            write!(f, ", write ")?;
            fmt_bytes(self.max_write, f)
        }
    }
}

/// Transparent instrumentation wrapper.
pub struct IOCounter<'a, IO: RimIO + ?Sized> {
    inner: &'a mut IO,
    pub stats: IoStats,
    /// Optional: local "block" alignment (e.g., 512, 4096, cluster_size...)
    pub align: u64,
}

impl<'a, IO: RimIO + ?Sized> IOCounter<'a, IO> {
    #[inline]
    pub fn new(inner: &'a mut IO) -> Self {
        Self {
            inner,
            stats: IoStats::default(),
            align: 1,
        }
    }

    #[inline]
    pub fn with_align(inner: &'a mut IO, align: u64) -> Self {
        let align = if align == 0 { 1 } else { align };
        Self {
            inner,
            stats: IoStats::default(),
            align,
        }
    }

    #[inline]
    pub fn snapshot(&self) -> IoStats {
        self.stats
    }
    #[inline]
    pub fn into_inner(self) -> &'a mut IO {
        self.inner
    }
}

impl<'a, IO: RimIO + ?Sized> RimIO for IOCounter<'a, IO> {
    #[inline]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let aligned =
            offset.is_multiple_of(self.align) && (data.len() as u64).is_multiple_of(self.align);
        if aligned {
            self.stats.aligned_writes += 1;
        } else {
            self.stats.unaligned_writes += 1;
        }

        self.stats.writes += 1;
        self.stats.write_bytes += data.len() as u64;
        if self.stats.max_write < data.len() as u64 {
            self.stats.max_write = data.len() as u64;
        }

        self.inner.write_at(offset, data)
    }

    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let aligned =
            offset.is_multiple_of(self.align) && (buf.len() as u64).is_multiple_of(self.align);
        if aligned {
            self.stats.aligned_reads += 1;
        } else {
            self.stats.unaligned_reads += 1;
        }

        self.stats.reads += 1;
        self.stats.read_bytes += buf.len() as u64;
        if self.stats.max_read < buf.len() as u64 {
            self.stats.max_read = buf.len() as u64;
        }

        self.inner.read_at(offset, buf)
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        self.stats.flushes += 1;
        self.inner.flush()
    }

    #[inline]
    fn set_offset(&mut self, p: u64) -> u64 {
        self.inner.set_offset(p)
    }
    #[inline]
    fn partition_offset(&self) -> u64 {
        self.inner.partition_offset()
    }
}

pub trait IOTracer {
    fn on_read(&mut self, _off: u64, _len: usize) {}
    fn on_write(&mut self, _off: u64, _len: usize) {}
    fn on_flush(&mut self) {}
}

pub struct TracingIO<'a, IO: RimIO + ?Sized, Tr: IOTracer> {
    inner: &'a mut IO,
    tracer: Tr,
}

impl<'a, IO: RimIO + ?Sized, Tr: IOTracer> RimIO for TracingIO<'a, IO, Tr> {
    fn write_at(&mut self, off: u64, data: &[u8]) -> RimIOResult {
        self.tracer.on_write(off, data.len());
        self.inner.write_at(off, data)
    }
    fn read_at(&mut self, off: u64, buf: &mut [u8]) -> RimIOResult {
        self.tracer.on_read(off, buf.len());
        self.inner.read_at(off, buf)
    }
    fn flush(&mut self) -> RimIOResult {
        self.tracer.on_flush();
        self.inner.flush()
    }
    fn set_offset(&mut self, p: u64) -> u64 {
        self.inner.set_offset(p)
    }
    fn partition_offset(&self) -> u64 {
        self.inner.partition_offset()
    }
}
