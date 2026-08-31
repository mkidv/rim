// SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec;

// Core modules
pub mod errors;
pub mod extent;
mod macros;
pub mod run;
pub mod stats;
pub mod utils;

// Backend modules
mod mem;
#[cfg(feature = "alloc")]
mod sparse;

#[cfg(feature = "std")]
mod std;

#[cfg(feature = "uefi")]
mod uefi;

#[cfg(feature = "mmap")]
mod mmap;

#[cfg(test)]
pub mod test_suite;

// Prelude re-exports (central entrypoint)
pub mod prelude {
    pub use super::RimIO;
    pub use super::RimIOExt;
    pub use super::RimIOSetLen;
    pub use super::RimIOStreamExt;
    pub use super::RimIOStructExt;
    pub use super::RimRead;
    pub use super::RimReadExt;
    pub use super::RimReadStructExt;
    pub use super::RimWrite;
    pub use super::RimWriteExt;
    pub use super::RimWriteStructExt;
    pub use super::copy_range;
    pub use super::errors::*;
    #[cfg(feature = "alloc")]
    pub use super::extent::{ExtentRimRead, IoExtent};
    #[cfg(feature = "alloc")]
    pub use super::mem::VecRimIO;
    pub use super::mem::{BoundedRimIO, MemRimIO, SliceRimIO};
    pub use super::run::*;
    #[cfg(feature = "alloc")]
    pub use super::sparse::{PagedSparseRimIO, SparseRimIO};
    pub use super::stats::*;
    pub use super::utils::*;

    #[cfg(feature = "std")]
    pub use super::std::{FileRimIO, ReadOnlyFileRimIO, StdRimIO};

    #[cfg(feature = "uefi")]
    pub use super::uefi::UefiRimIO;

    #[cfg(feature = "mmap")]
    pub use super::mmap::MmapRimIO;
}

// Re-export errors and memory backends
#[cfg(feature = "alloc")]
pub use extent::{ExtentRimRead, IoExtent};
#[cfg(feature = "alloc")]
pub use mem::VecRimIO;
pub use mem::{BoundedRimIO, MemRimIO, SliceRimIO};
pub use prelude::*;
#[cfg(feature = "std")]
pub use std::{FileRimIO, ReadOnlyFileRimIO, StdRimIO};
#[cfg(feature = "uefi")]
pub use uefi::UefiRimIO;

// Constants

/// Maximum size of internal scratch buffer (used for limited stack/chunking).
/// - `std`: 512 Kib (larger stack available, better throughput)
/// - `no_std`: 4 KiB (conservative for limited stack)
#[cfg(feature = "std")]
pub const BLOCK_BUF_SIZE: usize = 1024 * 1024;
#[cfg(not(feature = "std"))]
pub const BLOCK_BUF_SIZE: usize = 4096;

#[inline]
pub(crate) fn checked_add_offset(base: u64, offset: u64) -> RimIOResult<u64> {
    base.checked_add(offset).ok_or(RimIOError::OutOfBounds)
}

// Capability Traits

/// Read-only random-access I/O capability.
///
/// Implements exact-read contract: `read_at` must fill `buf` completely
/// or return an error (`OutOfBounds` / `Invalid` / `Other`).
pub trait RimRead {
    /// Reads `buf.len()` bytes into `buf` from `offset` (absolute or relative to current partition).
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult;

    /// Returns the total size of the storage/source in bytes, if known.
    fn total_size(&mut self) -> RimIOResult<u64> {
        Err(RimIOError::Unsupported)
    }
}

impl<R: RimRead + ?Sized> RimRead for &mut R {
    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        (**self).read_at(offset, buf)
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        (**self).total_size()
    }
}

#[cfg(feature = "alloc")]
impl<R: RimRead + ?Sized> RimRead for alloc::boxed::Box<R> {
    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        (**self).read_at(offset, buf)
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        (**self).total_size()
    }
}

/// Write random-access I/O capability.
pub trait RimWrite {
    /// Writes `data` starting at `offset` (absolute or relative to current partition).
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult;

    /// Marks a region as zero-filled.
    ///
    /// Backends may override this to avoid physically materializing zeroes.
    fn zero_at(&mut self, offset: u64, mut len: u64) -> RimIOResult {
        const ZERO_BUF: [u8; 4096] = [0u8; 4096];

        let mut current = offset;

        while len > 0 {
            let chunk = len.min(ZERO_BUF.len() as u64) as usize;

            self.write_at(current, &ZERO_BUF[..chunk])?;

            current = current
                .checked_add(chunk as u64)
                .ok_or(RimIOError::OutOfBounds)?;

            len -= chunk as u64;
        }

        Ok(())
    }

    /// Flushes any buffered data (may be a no-op).
    fn flush(&mut self) -> RimIOResult;
}

impl<W: RimWrite + ?Sized> RimWrite for &mut W {
    #[inline]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        (**self).write_at(offset, data)
    }

    #[inline]
    fn zero_at(&mut self, offset: u64, len: u64) -> RimIOResult {
        (**self).zero_at(offset, len)
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        (**self).flush()
    }
}

#[cfg(feature = "alloc")]
impl<W: RimWrite + ?Sized> RimWrite for alloc::boxed::Box<W> {
    #[inline]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        (**self).write_at(offset, data)
    }

    #[inline]
    fn zero_at(&mut self, offset: u64, len: u64) -> RimIOResult {
        (**self).zero_at(offset, len)
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        (**self).flush()
    }
}

/// Full read-write block IO abstraction trait with partition offset tracking.
///
/// Implementations may target RAM, files, block devices, UEFI, BIOS, etc.
pub trait RimIO: RimRead + RimWrite {
    fn set_offset(&mut self, partition_offset: u64) -> u64;
    fn partition_offset(&self) -> u64;
}

impl<IO: RimIO + ?Sized> RimIO for &mut IO {
    #[inline]
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        (**self).set_offset(partition_offset)
    }

    #[inline]
    fn partition_offset(&self) -> u64 {
        (**self).partition_offset()
    }
}

#[cfg(feature = "alloc")]
impl<IO: RimIO + ?Sized> RimIO for alloc::boxed::Box<IO> {
    #[inline]
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        (**self).set_offset(partition_offset)
    }

    #[inline]
    fn partition_offset(&self) -> u64 {
        (**self).partition_offset()
    }
}

/// Copies a range of bytes from a `RimRead` source to a `RimWrite` destination
/// using caller-provided scratch storage.
pub fn copy_range(
    src: &mut dyn RimRead,
    dest: &mut dyn RimWrite,
    src_offset: u64,
    dest_offset: u64,
    mut len: u64,
    scratch: &mut [u8],
) -> RimIOResult {
    if scratch.is_empty() {
        return Err(RimIOError::InvalidBuffer);
    }
    let mut s_off = src_offset;
    let mut d_off = dest_offset;

    while len > 0 {
        let chunk_size = len.min(scratch.len() as u64) as usize;
        src.read_at(s_off, &mut scratch[..chunk_size])?;
        dest.write_at(d_off, &scratch[..chunk_size])?;

        len -= chunk_size as u64;
        s_off += chunk_size as u64;
        d_off += chunk_size as u64;
    }
    Ok(())
}

/// Extension helpers for any `RimRead` storage.
pub trait RimReadExt: RimRead {
    #[cfg(feature = "std")]
    fn read_to_vec(&mut self, offset: u64, len: usize) -> RimIOResult<::std::vec::Vec<u8>> {
        let mut buf = ::std::vec![0u8; len];
        self.read_at(offset, &mut buf)?;
        Ok(buf)
    }

    #[cfg(feature = "std")]
    fn read_to_string(&mut self, offset: u64, len: usize) -> RimIOResult<String> {
        let vec = self.read_to_vec(offset, len)?;
        String::from_utf8(vec).map_err(|_| RimIOError::Other("Invalid UTF-8 sequence"))
    }

    /// Reads `buf.len()` bytes from `offset` in chunks of `chunk_size` or less.
    #[inline(always)]
    fn read_in_chunks(&mut self, offset: u64, buf: &mut [u8], chunk_size: usize) -> RimIOResult {
        let mut remaining = buf.len();
        let mut off = offset;
        let mut pos = 0;

        while remaining > 0 {
            let to_read = remaining.min(chunk_size);
            self.read_at(off, &mut buf[pos..pos + to_read])?;
            off += to_read as u64;
            pos += to_read;
            remaining -= to_read;
        }

        Ok(())
    }

    /// Reads a block or range of blocks of `block_size` starting at `offset`.
    #[inline(always)]
    fn read_block_best_effort(
        &mut self,
        offset: u64,
        buf: &mut [u8],
        block_size: usize,
    ) -> RimIOResult {
        if offset.is_multiple_of(block_size as u64) && buf.len().is_multiple_of(block_size) {
            self.read_at(offset, buf)
        } else {
            self.read_in_chunks(offset, buf, BLOCK_BUF_SIZE)
        }
    }

    /// Optimized multi-read (batch read) for FS clusters or blocks.
    #[inline(always)]
    fn read_multi_at(
        &mut self,
        offsets: &[u64],
        cluster_size: usize,
        buf: &mut [u8],
    ) -> RimIOResult {
        if buf.len() != offsets.len() * cluster_size {
            return Err(RimIOError::Invalid("read_multi_at: buffer length mismatch"));
        }

        if offsets.is_empty() {
            return Ok(());
        }

        let mut current_start_idx = 0;
        let mut current_run_len = 1;

        for i in 1..offsets.len() {
            let prev_offset = offsets[i - 1];
            let curr_offset = offsets[i];

            if curr_offset == prev_offset + cluster_size as u64 {
                current_run_len += 1;
            } else {
                let run_bytes = current_run_len * cluster_size;
                let buf_start = current_start_idx * cluster_size;
                let buf_end = buf_start + run_bytes;

                self.read_at(offsets[current_start_idx], &mut buf[buf_start..buf_end])?;

                current_start_idx = i;
                current_run_len = 1;
            }
        }

        let run_bytes = current_run_len * cluster_size;
        let buf_start = current_start_idx * cluster_size;
        let buf_end = buf_start + run_bytes;
        self.read_at(offsets[current_start_idx], &mut buf[buf_start..buf_end])?;

        Ok(())
    }

    RimRead_impl_primitive_r!(u16, u32, u64, u128);
}

impl<T: RimRead + ?Sized> RimReadExt for T {}

/// Extension helpers for any `RimWrite` destination.
pub trait RimWriteExt: RimWrite {
    /// Writes `buf.len()` bytes at `offset` in chunks of `chunk_size` or less.
    #[inline(always)]
    fn write_in_chunks(&mut self, offset: u64, buf: &[u8], chunk_size: usize) -> RimIOResult {
        let mut remaining = buf.len();
        let mut off = offset;
        let mut pos = 0;

        while remaining > 0 {
            let to_write = remaining.min(chunk_size);
            self.write_at(off, &buf[pos..pos + to_write])?;
            off += to_write as u64;
            pos += to_write;
            remaining -= to_write;
        }

        Ok(())
    }

    /// Writes a block or range of blocks of `block_size` starting at `offset`.
    #[inline(always)]
    fn write_block_best_effort(
        &mut self,
        offset: u64,
        buf: &[u8],
        block_size: usize,
    ) -> RimIOResult {
        if offset.is_multiple_of(block_size as u64) && buf.len().is_multiple_of(block_size) {
            self.write_at(offset, buf)
        } else {
            self.write_in_chunks(offset, buf, BLOCK_BUF_SIZE)
        }
    }

    /// Optimized multi-write (batch write) for FS clusters or blocks.
    #[inline(always)]
    fn write_multi_at(&mut self, offsets: &[u64], cluster_size: usize, buf: &[u8]) -> RimIOResult {
        if buf.len() != offsets.len() * cluster_size {
            return Err(RimIOError::Invalid(
                "write_multi_at: buffer length mismatch",
            ));
        }

        if offsets.is_empty() {
            return Ok(());
        }

        let mut current_start_idx = 0;
        let mut current_run_len = 1;

        for i in 1..offsets.len() {
            let prev_offset = offsets[i - 1];
            let curr_offset = offsets[i];

            if curr_offset == prev_offset + cluster_size as u64 {
                current_run_len += 1;
            } else {
                let run_bytes = current_run_len * cluster_size;
                let buf_start = current_start_idx * cluster_size;
                let buf_end = buf_start + run_bytes;

                self.write_at(offsets[current_start_idx], &buf[buf_start..buf_end])?;

                current_start_idx = i;
                current_run_len = 1;
            }
        }

        let run_bytes = current_run_len * cluster_size;
        let buf_start = current_start_idx * cluster_size;
        let buf_end = buf_start + run_bytes;
        self.write_at(offsets[current_start_idx], &buf[buf_start..buf_end])?;

        Ok(())
    }

    /// Fills a region with zeroes.
    #[inline(always)]
    fn zero_fill(&mut self, offset: u64, len: usize) -> RimIOResult {
        self.zero_at(offset, len as u64)
    }

    /// Copies data from a source `RimRead` into this one using a provided buffer.
    fn copy_from_using_buffer(
        &mut self,
        src: &mut dyn RimRead,
        src_offset: u64,
        dest_offset: u64,
        mut len: u64,
        buf: &mut [u8],
    ) -> RimIOResult {
        if buf.is_empty() {
            return Err(RimIOError::InvalidBuffer);
        }
        let mut s_off = src_offset;
        let mut d_off = dest_offset;

        while len > 0 {
            let to_process = len.min(buf.len() as u64) as usize;
            src.read_at(s_off, &mut buf[..to_process])?;
            self.write_at(d_off, &buf[..to_process])?;

            len -= to_process as u64;
            s_off += to_process as u64;
            d_off += to_process as u64;
        }
        Ok(())
    }

    /// Copies data from a source `RimRead` into this one.
    fn copy_from(
        &mut self,
        src: &mut dyn RimRead,
        src_offset: u64,
        dest_offset: u64,
        len: u64,
    ) -> RimIOResult {
        #[cfg(feature = "alloc")]
        {
            #[cfg(feature = "std")]
            const CHUNK_SIZE: usize = 1024 * 1024;
            #[cfg(not(feature = "std"))]
            const CHUNK_SIZE: usize = 64 * 1024;
            let mut buf = alloc::vec![0u8; CHUNK_SIZE];
            self.copy_from_using_buffer(src, src_offset, dest_offset, len, &mut buf)
        }
        #[cfg(not(feature = "alloc"))]
        {
            let mut buf = [0u8; BLOCK_BUF_SIZE];
            self.copy_from_using_buffer(src, src_offset, dest_offset, len, &mut buf)
        }
    }

    /// Copies data from a source `RimRead` into this one, notifying byte progress via a closure.
    fn copy_from_with_progress<F: FnMut(u64, u64)>(
        &mut self,
        src: &mut dyn RimRead,
        src_offset: u64,
        dest_offset: u64,
        mut len: u64,
        mut on_progress: F,
    ) -> RimIOResult {
        #[cfg(feature = "std")]
        const CHUNK_SIZE: usize = 1024 * 1024;
        #[cfg(all(feature = "alloc", not(feature = "std")))]
        const CHUNK_SIZE: usize = 64 * 1024;
        #[cfg(not(feature = "alloc"))]
        const CHUNK_SIZE: usize = BLOCK_BUF_SIZE;

        #[cfg(feature = "alloc")]
        let mut buf = alloc::vec![0u8; CHUNK_SIZE];
        #[cfg(not(feature = "alloc"))]
        let mut buf = [0u8; BLOCK_BUF_SIZE];

        let total = len;
        let mut copied = 0u64;
        let mut s_off = src_offset;
        let mut d_off = dest_offset;

        while len > 0 {
            let to_process = len.min(CHUNK_SIZE as u64) as usize;
            src.read_at(s_off, &mut buf[..to_process])?;
            self.write_at(d_off, &buf[..to_process])?;

            len -= to_process as u64;
            copied += to_process as u64;
            s_off += to_process as u64;
            d_off += to_process as u64;

            on_progress(copied, total);
        }
        Ok(())
    }

    RimWrite_impl_primitive_w!(u16, u32, u64, u128);
}

impl<T: RimWrite + ?Sized> RimWriteExt for T {}

/// Extension helpers combining `RimReadExt` and `RimWriteExt` for `RimIO`.
pub trait RimIOExt: RimReadExt + RimWriteExt {}

impl<T: RimIO + ?Sized> RimIOExt for T {}

#[inline]
fn validate_stream_chunk<const N: usize>(chunk: usize) -> RimIOResult {
    if N == 0 {
        return Err(RimIOError::Invalid(
            "element size must be greater than zero",
        ));
    }
    if chunk == 0 {
        return Err(RimIOError::Invalid("chunk size must be greater than zero"));
    }

    chunk
        .checked_mul(N)
        .map(|_| ())
        .ok_or(RimIOError::Invalid("chunk size overflow"))
}

#[inline(always)]
fn array_ref_from_slice<const N: usize>(slice: &[u8]) -> &[u8; N] {
    debug_assert_eq!(slice.len(), N);
    // SAFETY: all callers slice exactly `N` bytes (e.g. `start..start + N`).
    unsafe { &*slice.as_ptr().cast::<[u8; N]>() }
}

#[cfg(not(feature = "alloc"))]
#[inline]
fn validate_no_alloc_chunk<const N: usize>(chunk: usize) -> RimIOResult {
    validate_stream_chunk::<N>(chunk)?;

    let entries_per_chunk = BLOCK_BUF_SIZE / N;
    if chunk > entries_per_chunk {
        return Err(RimIOError::Invalid("chunk too large for internal buffer"));
    }

    Ok(())
}

pub trait RimIOStreamExt: RimIO {
    /// Stream-read N-byte fixed-size elements using a callback function (e.g. for u16, u32, custom entries).
    fn read_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize, &[u8; N]);

    /// Stream-write N-byte fixed-size elements using a generator function (e.g. for u16, u32, custom entries).
    fn write_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize) -> [u8; N];

    /// Stream-read fixed-size elements at multiple arbitrary offsets using a callback.
    fn read_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize, &[u8; N]);

    /// Stream-write fixed-size elements at multiple arbitrary offsets using a generator callback.
    fn write_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize) -> [u8; N];
}

#[cfg(feature = "alloc")]
impl<T: RimIO + ?Sized> RimIOStreamExt for T {
    #[inline]
    fn read_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize, &[u8; N]),
    {
        validate_stream_chunk::<N>(chunk)?;
        let mut buf = vec![0u8; chunk * N];

        let mut remaining = count;
        let mut current_offset = offset;
        let mut index = 0;

        while remaining > 0 {
            let to_read = remaining.min(chunk);
            let bytes_to_read = to_read * N;
            self.read_in_chunks(current_offset, &mut buf[..bytes_to_read], BLOCK_BUF_SIZE)?;

            for i in 0..to_read {
                let start = i * N;
                let slice = &buf[start..start + N];
                f(index, array_ref_from_slice::<N>(slice));
                index += 1;
            }

            current_offset += bytes_to_read as u64;
            remaining -= to_read;
        }

        Ok(())
    }

    #[inline]
    fn write_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize) -> [u8; N],
    {
        validate_stream_chunk::<N>(chunk)?;
        let mut buf = vec![0u8; chunk * N];

        let mut remaining = count;
        let mut current_offset = offset;
        let mut index = 0;

        while remaining > 0 {
            let to_write = remaining.min(chunk);
            let bytes_to_write = to_write * N;

            for i in 0..to_write {
                buf[i * N..(i + 1) * N].copy_from_slice(&f(index));
                index += 1;
            }

            self.write_in_chunks(current_offset, &buf[..bytes_to_write], BLOCK_BUF_SIZE)?;

            current_offset += bytes_to_write as u64;
            remaining -= to_write;
        }

        Ok(())
    }

    /// Stream-read fixed-size elements at multiple arbitrary offsets using a callback.
    #[inline]
    fn read_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize, &[u8; N]),
    {
        validate_stream_chunk::<N>(chunk)?;
        let mut buf = vec![0u8; chunk * N];

        for (chunk_idx, offset_chunk) in offsets.chunks(chunk).enumerate() {
            let to_read = offset_chunk.len();

            for (i, &off) in offset_chunk.iter().enumerate() {
                let buf_slice = &mut buf[i * N..(i + 1) * N];
                self.read_at(off, buf_slice)?;
            }

            for i in 0..to_read {
                let slice = &buf[i * N..(i + 1) * N];
                f(chunk_idx * chunk + i, array_ref_from_slice::<N>(slice));
            }
        }

        Ok(())
    }

    /// Stream-write fixed-size elements at multiple arbitrary offsets using a generator callback.
    #[inline]
    fn write_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize) -> [u8; N],
    {
        validate_stream_chunk::<N>(chunk)?;
        let mut buf = vec![0u8; chunk * N];

        for (chunk_idx, offset_chunk) in offsets.chunks(chunk).enumerate() {
            let to_write = offset_chunk.len();

            for i in 0..to_write {
                let val = f(chunk_idx * chunk + i);
                buf[i * N..(i + 1) * N].copy_from_slice(&val);
            }

            for (i, &off) in offset_chunk.iter().enumerate() {
                let buf_slice = &buf[i * N..(i + 1) * N];
                self.write_at(off, buf_slice)?;
            }
        }

        Ok(())
    }
}

#[cfg(not(feature = "alloc"))]
impl<T: RimIO + ?Sized> RimIOStreamExt for T {
    #[inline]
    fn read_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize, &[u8; N]),
    {
        validate_no_alloc_chunk::<N>(chunk)?;
        let mut buf = [0u8; BLOCK_BUF_SIZE];

        let mut remaining = count;
        let mut current_offset = offset;
        let mut index = 0;

        while remaining > 0 {
            let to_read = remaining.min(chunk);
            let bytes_to_read = to_read * N;
            self.read_in_chunks(current_offset, &mut buf[..bytes_to_read], BLOCK_BUF_SIZE)?;

            for i in 0..to_read {
                let start = i * N;
                let slice = &buf[start..start + N];
                f(index, array_ref_from_slice::<N>(slice));
                index += 1;
            }

            current_offset += bytes_to_read as u64;
            remaining -= to_read;
        }

        Ok(())
    }

    #[inline]
    fn write_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize) -> [u8; N],
    {
        validate_no_alloc_chunk::<N>(chunk)?;
        let mut buf = [0u8; BLOCK_BUF_SIZE];

        let mut remaining = count;
        let mut current_offset = offset;
        let mut index = 0;

        while remaining > 0 {
            let to_write = remaining.min(chunk);
            let bytes_to_write = to_write * N;

            for i in 0..to_write {
                buf[i * N..(i + 1) * N].copy_from_slice(&f(index));
                index += 1;
            }

            self.write_in_chunks(current_offset, &buf[..bytes_to_write], BLOCK_BUF_SIZE)?;

            current_offset += bytes_to_write as u64;
            remaining -= to_write;
        }

        Ok(())
    }

    /// Stream-read fixed-size elements at multiple arbitrary offsets (no-alloc).
    #[inline]
    fn read_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        _chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize, &[u8; N]),
    {
        validate_no_alloc_chunk::<N>(1)?;
        let mut elem = [0u8; N];

        for (i, &off) in offsets.iter().enumerate() {
            self.read_at(off, &mut elem)?;
            f(i, &elem);
        }
        Ok(())
    }

    /// Stream-write fixed-size elements at multiple arbitrary offsets (no-alloc).
    #[inline]
    fn write_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        _chunk: usize,
        mut f: F,
    ) -> RimIOResult
    where
        F: FnMut(usize) -> [u8; N],
    {
        validate_no_alloc_chunk::<N>(1)?;

        for (i, &off) in offsets.iter().enumerate() {
            let bytes = f(i);
            self.write_at(off, &bytes)?;
        }
        Ok(())
    }
}

/// Trait for setting the length of a RimIO object.
///
/// Allows resizing the underlying storage (if supported by the backend).
pub trait RimIOSetLen: RimIO {
    /// Sets the length of the storage.
    fn set_len(&mut self, len: u64) -> RimIOResult;
}

/// Extension trait for reading structs using zerocopy from any `RimRead` source.
pub trait RimReadStructExt: RimRead {
    /// Reads a struct of type `T` from the given offset.
    fn read_struct<T: zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable>(
        &mut self,
        offset: u64,
    ) -> RimIOResult<T> {
        let size = core::mem::size_of::<T>();

        #[cfg(feature = "alloc")]
        {
            let mut buf = alloc::vec![0u8; size];
            self.read_at(offset, &mut buf)?;
            T::read_from_bytes(&buf).map_err(|_| RimIOError::Other("read_struct failed"))
        }

        #[cfg(not(feature = "alloc"))]
        {
            assert!(size <= BLOCK_BUF_SIZE, "read_struct: type too large");
            let mut buf = [0u8; BLOCK_BUF_SIZE];
            self.read_at(offset, &mut buf[..size])?;
            T::read_from_bytes(&buf[..size]).map_err(|_| RimIOError::Other("read_struct failed"))
        }
    }
}

impl<T: RimRead + ?Sized> RimReadStructExt for T {}

/// Extension trait for writing structs using zerocopy to any `RimWrite` destination.
pub trait RimWriteStructExt: RimWrite {
    /// Writes a struct of type `T` at the given offset.
    fn write_struct<T: zerocopy::IntoBytes + zerocopy::KnownLayout + zerocopy::Immutable>(
        &mut self,
        offset: u64,
        val: &T,
    ) -> RimIOResult {
        let bytes = val.as_bytes();
        self.write_at(offset, bytes)
    }
}

impl<T: RimWrite + ?Sized> RimWriteStructExt for T {}

/// Extension trait combining read and write struct helpers for `RimIO`.
pub trait RimIOStructExt: RimReadStructExt + RimWriteStructExt {}

impl<T: RimIO + ?Sized> RimIOStructExt for T {}
