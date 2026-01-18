// SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::vec;

// Core modules
pub mod errors;
mod macros;
pub mod stats;
pub mod utils;

// Backend modules
#[cfg(feature = "mem")]
mod mem;

#[cfg(feature = "std")]
mod std;

#[cfg(feature = "uefi")]
mod uefi;

// Prelude re-exports (central entrypoint)
pub mod prelude {
    pub use super::RimIO;
    pub use super::RimIOExt;
    pub use super::RimIOSetLen;
    pub use super::RimIOStreamExt;
    pub use super::RimIOStructExt;
    pub use super::errors::*;
    pub use super::stats::*;

    #[cfg(feature = "mem")]
    pub use super::mem::MemRimIO;

    #[cfg(feature = "std")]
    pub use super::std::StdRimIO;

    #[cfg(feature = "uefi")]
    pub use super::uefi::UefiRimIO;
}

// Internal use
use errors::*;

// Constants

/// Maximum size of internal scratch buffer (used for streaming/chunked ops).
/// 4 KiB = typical page size and common disk sector/cluster size.
/// Safe for no_std/UEFI stack usage, overridable in high-level code.
pub const BLOCK_BUF_SIZE: usize = 4096;

// Traits

/// Block IO abstraction trait.
///
/// Allows read/write/flush at arbitrary offsets.
/// Implementations may target RAM, files, block devices, UEFI, BIOS, etc.
pub trait RimIO {
    /// Writes `data` at `offset` (absolute).
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult;

    /// Reads `buf.len()` bytes into `buf` from `offset` (absolute).
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult;
    /// Flushes any buffered data (may be a no-op).
    fn flush(&mut self) -> RimIOResult;
    fn set_offset(&mut self, partition_offset: u64) -> u64;
    fn partition_offset(&self) -> u64;

    /// Copies data from a source `RimIO` into this one.
    ///
    /// The default implementation uses an intermediate buffer (double-copy).
    /// Specialized implementations (like `MemRimIO`) can override this to
    /// read directly from `src` into their own storage (single-copy).
    #[cfg(feature = "alloc")]
    fn copy_from(
        &mut self,
        src: &mut dyn RimIO,
        src_offset: u64,
        dest_offset: u64,
        mut len: u64,
    ) -> RimIOResult {
        // Default: Large Heap Buffer (Double Copy)
        const CHUNK_SIZE: usize = 64 * 1024; // 64 KiB
        let mut buf = alloc::vec![0u8; CHUNK_SIZE];
        let mut s_off = src_offset;
        let mut d_off = dest_offset;

        while len > 0 {
            let to_process = len.min(CHUNK_SIZE as u64) as usize;
            src.read_at(s_off, &mut buf[..to_process])?;
            self.write_at(d_off, &buf[..to_process])?;

            len -= to_process as u64;
            s_off += to_process as u64;
            d_off += to_process as u64;
        }
        Ok(())
    }

    #[cfg(not(feature = "alloc"))]
    fn copy_from(
        &mut self,
        src: &mut dyn RimIO,
        src_offset: u64,
        dest_offset: u64,
        mut len: u64,
    ) -> RimIOResult {
        // Default: Stack Buffer (Double Copy for no_std)
        let mut buf = [0u8; BLOCK_BUF_SIZE];
        let mut s_off = src_offset;
        let mut d_off = dest_offset;

        while len > 0 {
            let to_process = len.min(BLOCK_BUF_SIZE as u64) as usize;
            src.read_at(s_off, &mut buf[..to_process])?;
            self.write_at(d_off, &buf[..to_process])?;

            len -= to_process as u64;
            s_off += to_process as u64;
            d_off += to_process as u64;
        }
        Ok(())
    }
}

/// Extension helpers for RimIO.
///
/// Provides optimized or convenient helpers:
/// - aligned reads/writes
/// - multi-block operations
/// - low-level write helpers (write_u16/32/64)
/// - streamed reads/writes
/// - zero fill, primitive writes
pub trait RimIOExt: RimIO {
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
    /// Reads a block or range of blocks of `block_size` starting at `offset`.
    ///
    /// If offset and length are aligned to `block_size`, performs a single fast read.
    /// Otherwise, falls back to reading block by block.
    ///
    /// Useful for FS implementations (cluster/block reads).
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

    /// Writes a block or range of blocks of `block_size` starting at `offset`.
    ///
    /// If offset and length are aligned to `block_size`, performs a single fast write.
    /// Otherwise, falls back to writing block by block.
    ///
    /// Useful for FS implementations (cluster/block writes).
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

    /// Optimized multi-read (batch read) for FS clusters or blocks.
    ///
    /// This default implementation attempts to coalesce adjacent reads into larger transactions
    /// to reduce overhead. It typically performs better than individual `read_at` calls.
    ///
    /// # Errors
    /// Returns `RimIOError::Invalid` if `buf.len()` does not match `offsets.len() * cluster_size`.
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

            // Check if contiguous
            if curr_offset == prev_offset + cluster_size as u64 {
                current_run_len += 1;
            } else {
                // Execute previous run
                let run_bytes = current_run_len * cluster_size;
                let buf_start = current_start_idx * cluster_size;
                let buf_end = buf_start + run_bytes;

                self.read_at(offsets[current_start_idx], &mut buf[buf_start..buf_end])?;

                // Start new run
                current_start_idx = i;
                current_run_len = 1;
            }
        }

        // Execute final run
        let run_bytes = current_run_len * cluster_size;
        let buf_start = current_start_idx * cluster_size;
        let buf_end = buf_start + run_bytes;
        self.read_at(offsets[current_start_idx], &mut buf[buf_start..buf_end])?;

        Ok(())
    }

    /// Optimized multi-write (batch write) for FS clusters or blocks.
    ///
    /// This default implementation attempts to coalesce adjacent writes into larger transactions.
    ///
    /// # Errors
    /// Returns `RimIOError::Invalid` if `buf.len()` does not match `offsets.len() * cluster_size`.
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

            // Check if contiguous
            if curr_offset == prev_offset + cluster_size as u64 {
                current_run_len += 1;
            } else {
                // Execute previous run
                let run_bytes = current_run_len * cluster_size;
                let buf_start = current_start_idx * cluster_size;
                let buf_end = buf_start + run_bytes;

                self.write_at(offsets[current_start_idx], &buf[buf_start..buf_end])?;

                // Start new run
                current_start_idx = i;
                current_run_len = 1;
            }
        }

        // Execute final run
        let run_bytes = current_run_len * cluster_size;
        let buf_start = current_start_idx * cluster_size;
        let buf_end = buf_start + run_bytes;
        self.write_at(offsets[current_start_idx], &buf[buf_start..buf_end])?;

        Ok(())
    }

    /// Fills a region with zeroes.
    ///
    /// Used for quick cluster clearing, FS formatting, VBR/FSInfo clears, etc.
    #[inline(always)]
    fn zero_fill(&mut self, offset: u64, len: usize) -> RimIOResult {
        const ZERO_BUF: [u8; BLOCK_BUF_SIZE] = [0u8; BLOCK_BUF_SIZE];
        let mut remaining = len;
        let mut off = offset;
        while remaining > 0 {
            let chunk = remaining.min(ZERO_BUF.len());
            self.write_at(off, &ZERO_BUF[..chunk])?;
            off += chunk as u64;
            remaining -= chunk;
        }
        Ok(())
    }

    // Implements read/write helpers for primitive types (u16, u32, u64, u128)
    RimIO_impl_primitive_rw!(u16, u32, u64, u128);
}

impl<T: RimIO + ?Sized> RimIOExt for T {}

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
                f(index, slice.try_into().unwrap());
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
        let mut buf = vec![0u8; chunk * N];

        for (chunk_idx, offset_chunk) in offsets.chunks(chunk).enumerate() {
            let to_read = offset_chunk.len();

            for (i, &off) in offset_chunk.iter().enumerate() {
                let buf_slice = &mut buf[i * N..(i + 1) * N];
                self.read_at(off, buf_slice)?;
            }

            for i in 0..to_read {
                let slice = &buf[i * N..(i + 1) * N];
                let array_ref: &[u8; N] = slice.try_into().expect("slice len mismatch");
                f(chunk_idx * chunk + i, array_ref);
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
        const BUF_SIZE: usize = BLOCK_BUF_SIZE;
        let mut buf = [0u8; BUF_SIZE];
        let entries_per_chunk = BUF_SIZE / N;
        assert!(
            chunk <= entries_per_chunk,
            "Chunk too large for the internal buffer."
        );

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
                f(index, slice.try_into().unwrap());
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
        const BUF_SIZE: usize = BLOCK_BUF_SIZE;
        let mut buf = [0u8; BUF_SIZE];
        let entries_per_chunk = BUF_SIZE / N;
        assert!(
            chunk <= entries_per_chunk,
            "Chunk too large for the internal buffer."
        );

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
        let mut elem = [0u8; N];
        assert!(N <= BLOCK_BUF_SIZE, "N too large for internal buffer.",);

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
        assert!(N <= BLOCK_BUF_SIZE, "N too large for internal buffer.",);

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

/// Extension trait for reading and writing structs using zerocopy.
///
/// Provides helpers to read a struct from a given offset and write a struct at a given offset.
/// Requires the struct to implement zerocopy traits for safe conversion.
pub trait RimIOStructExt: RimIO {
    /// Reads a struct of type `T` from the given offset.
    fn read_struct<T: zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable>(
        &mut self,
        offset: u64,
    ) -> RimIOResult<T> {
        let size = core::mem::size_of::<T>();
        assert!(size <= BLOCK_BUF_SIZE, "read_struct: type too large");
        let mut buf = [0u8; BLOCK_BUF_SIZE];
        self.read_at(offset, &mut buf[..size])?;
        T::read_from_bytes(&buf[..size]).map_err(|_| RimIOError::Other("read_struct failed"))
    }

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

impl<T: RimIO + ?Sized> RimIOStructExt for T {}
