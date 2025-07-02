// SPDX-License-Identifier: MIT
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

// === Core modules ===
pub mod error;
pub mod utils;
mod macros;

// === Backend modules ===
#[cfg(feature = "mem")]
mod mem;

#[cfg(feature = "std")]
mod std;

#[cfg(feature = "uefi")]
mod uefi;

// === Prelude re-exports (central entrypoint) ===
pub mod prelude {
    pub use super::BlockIO;
    pub use super::BlockIOExt;
    pub use super::BlockIOSetLen;
    pub use super::BlockIOStreamExt;
    pub use super::BlockIOStructExt;
    pub use super::error::*;

    #[cfg(feature = "mem")]
    pub use super::mem::MemBlockIO;

    #[cfg(feature = "std")]
    pub use super::std::StdBlockIO;

    #[cfg(feature = "uefi")]
    pub use super::uefi::UefiBlockIO;
}

// === Internal use ===
use error::*;
#[allow(clippy::single_component_path_imports)]
use paste;

// === Constants ===
/// Maximum size of internal scratch buffer (used for streaming/chunked ops)
const BLOCK_BUF_SIZE: usize = 8192;

// === Traits ===

/// Block IO abstraction trait.
///
/// Allows read/write/flush at arbitrary offsets.
/// Implementations may target RAM, files, block devices, UEFI, BIOS, etc.
pub trait BlockIO {
    /// Writes `data` at `offset` (absolute).
    fn write_at(&mut self, offset: u64, data: &[u8]) -> BlockIOResult;

    /// Reads `buf.len()` bytes into `buf` from `offset` (absolute).
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> BlockIOResult;
    /// Flushes any buffered data (may be a no-op).
    fn flush(&mut self) -> BlockIOResult;
    fn set_offset(&mut self, partition_offset: u64) -> u64;
    fn partition_offset(&self) -> u64;
}

/// Extension helpers for BlockIO.
///
/// Provides optimized or convenient helpers:
/// - aligned reads/writes
/// - multi-block operations
/// - low-level write helpers (write_u16/32/64)
/// - streamed reads/writes
/// - zero fill, primitive writes
pub trait BlockIOExt: BlockIO {
    /// Reads `buf.len()` bytes from `offset` in chunks of `chunk_size` or less.
    #[inline(always)]
    fn read_in_chunks(&mut self, offset: u64, buf: &mut [u8], chunk_size: usize) -> BlockIOResult {
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
    fn write_in_chunks(&mut self, offset: u64, buf: &[u8], chunk_size: usize) -> BlockIOResult {
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
    ) -> BlockIOResult {
        if offset % block_size as u64 == 0 && buf.len() % block_size == 0 {
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
    ) -> BlockIOResult {
        if offset % block_size as u64 == 0 && buf.len() % block_size == 0 {
            self.write_at(offset, buf)
        } else {
            self.write_in_chunks(offset, buf, BLOCK_BUF_SIZE)
        }
    }

    /// Optimized multi-read (batch read) for FS clusters or blocks.
    #[inline(always)]
    fn read_multi_at(
        &mut self,
        offsets: &[u64],
        cluster_size: usize,
        buf: &mut [u8],
    ) -> BlockIOResult {
        for (i, &off) in offsets.iter().enumerate() {
            let start = i * cluster_size;
            let end = start + cluster_size;
            self.read_at(off, &mut buf[start..end])?;
        }
        Ok(())
    }

    /// Optimized multi-write (batch write) for FS clusters or blocks.
    #[inline(always)]
    fn write_multi_at(
        &mut self,
        offsets: &[u64],
        cluster_size: usize,
        buf: &[u8],
    ) -> BlockIOResult {
        for (i, &off) in offsets.iter().enumerate() {
            let start = i * cluster_size;
            let end = start + cluster_size;
            self.write_at(off, &buf[start..end])?;
        }
        Ok(())
    }

    /// Fills a region with zeroes.
    ///
    /// Used for quick cluster clearing, FS formatting, VBR/FSInfo clears, etc.
    #[inline(always)]
    fn zero_fill(&mut self, offset: u64, len: usize) -> BlockIOResult {
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
    blockio_impl_primitive_rw!(u16, u32, u64, u128);
}

impl<T: BlockIO + ?Sized> BlockIOExt for T {}

pub trait BlockIOStreamExt: BlockIO {
    /// Stream-read N-byte fixed-size elements using a callback function (e.g. for u16, u32, custom entries).
    fn read_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        f: F,
    ) -> BlockIOResult
    where
        F: FnMut(usize, &[u8; N]);

    /// Stream-write N-byte fixed-size elements using a generator function (e.g. for u16, u32, custom entries).
    fn write_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        f: F,
    ) -> BlockIOResult
    where
        F: FnMut(usize) -> [u8; N];

    /// Stream-read fixed-size elements at multiple arbitrary offsets using a callback.
    fn read_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        f: F,
    ) -> BlockIOResult
    where
        F: FnMut(usize, &[u8; N]);

    /// Stream-write fixed-size elements at multiple arbitrary offsets using a generator callback.
    fn write_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        f: F,
    ) -> BlockIOResult
    where
        F: FnMut(usize) -> [u8; N];
}

#[cfg(feature = "alloc")]
impl<T: BlockIO + ?Sized> BlockIOStreamExt for T {
    fn read_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> BlockIOResult
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

    fn write_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> BlockIOResult
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
    #[inline(always)]
    fn read_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        mut f: F,
    ) -> BlockIOResult
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
    #[inline(always)]
    fn write_multi_streamed<const N: usize, F>(
        &mut self,
        offsets: &[u64],
        chunk: usize,
        mut f: F,
    ) -> BlockIOResult
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
impl<T: BlockIO + ?Sized> BlockIOStreamExt for T {
    fn read_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> BlockIOResult
    where
        F: FnMut(usize, &[u8; N]),
    {
        const BUF_SIZE: usize = BLOCK_BUF_SIZE;
        let mut buf = [0u8; BUF_SIZE];
        let entries_per_chunk = BUF_SIZE / N;
        assert!(
            chunk <= entries_per_chunk,
            "Chunk trop grand pour le buffer interne."
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

    fn write_chunks_streamed<const N: usize, F>(
        &mut self,
        offset: u64,
        count: usize,
        chunk: usize,
        mut f: F,
    ) -> BlockIOResult
    where
        F: FnMut(usize) -> [u8; N],
    {
        const BUF_SIZE: usize = BLOCK_BUF_SIZE;
        let mut buf = [0u8; BUF_SIZE];
        let entries_per_chunk = BUF_SIZE / N;
        assert!(
            chunk <= entries_per_chunk,
            "Chunk trop grand pour le buffer interne."
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
}

/// Trait for setting the length of a BlockIO object.
///
/// Allows resizing the underlying storage (if supported by the backend).
pub trait BlockIOSetLen {
    /// Sets the length of the storage.
    fn set_len(&mut self, len: u64) -> BlockIOResult;
}

/// Extension trait for reading and writing structs using zerocopy.
///
/// Provides helpers to read a struct from a given offset and write a struct at a given offset.
/// Requires the struct to implement zerocopy traits for safe conversion.
pub trait BlockIOStructExt: BlockIO {
    /// Reads a struct of type `T` from the given offset.
    fn read_struct<T: zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable>(
        &mut self,
        offset: u64,
    ) -> BlockIOResult<T> {
        let size = core::mem::size_of::<T>();
        assert!(size <= BLOCK_BUF_SIZE, "read_struct: type too large");
        let mut buf = [0u8; BLOCK_BUF_SIZE];
        self.read_at(offset, &mut buf[..size])?;
        T::read_from_bytes(&buf[..size]).map_err(|_| BlockIOError::Error("read_struct failed"))
    }

    /// Writes a struct of type `T` at the given offset.
    fn write_struct<T: zerocopy::IntoBytes + zerocopy::KnownLayout + zerocopy::Immutable>(
        &mut self,
        offset: u64,
        val: &T,
    ) -> BlockIOResult {
        let bytes = val.as_bytes();
        self.write_at(offset, bytes)
    }
}

impl<T: BlockIO + ?Sized> BlockIOStructExt for T {}
