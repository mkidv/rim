// SPDX-License-Identifier: MIT
use crate::{BlockIO, BlockIOError, BlockIOResult};

use uefi::boot::ScopedProtocol;
use uefi::proto::media::block::{BlockIO as UefiBlockIo, BlockIOMedia, Lba};

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::boxed::Box;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

/// UEFI BlockIO backend for `BlockIO`.
///
/// Works for arbitrary block sizes:
/// - Without `alloc`: supports block_size <= 512 (stack buf), otherwise `Unsupported`.
/// - With `alloc` uses stack up to 4KiB, heap beyond.
pub struct UefiBlockIO {
    blk: ScopedProtocol<UefiBlockIo>,
    partition_offset: u64,
}

impl UefiBlockIO {
     #[inline]
    pub fn new(blk: ScopedProtocol<UefiBlockIo>) -> Self {
        Self {
            blk,
            partition_offset: 0,
        }
    }

     #[inline]
    pub fn new_with_offset(blk: ScopedProtocol<UefiBlockIo>, partition_offset: u64) -> Self {
        Self {
            blk,
            partition_offset,
        }
    }

    #[inline(always)]
    fn media(&self) -> &BlockIOMedia {
        self.blk.media()
    }

    #[inline(always)]
    fn media_id(&self) -> u32 {
        self.media().media_id()
    }

    #[inline(always)]
    fn block_size(&self) -> usize {
        self.media().block_size() as usize
    }

    #[inline]
    fn media_len(&self) -> u64 {
        // last_block is inclusive → total bytes = (last_block + 1) * block_size
        (self.media().last_block() + 1) * self.block_size() as u64
    }

    #[inline]
    fn check_bounds(&self, abs_off: u64, len: usize) -> BlockIOResult {
        let end = abs_off
            .checked_add(len as u64)
            .ok_or(BlockIOError::OutOfBounds)?;
        if end > self.media_len() {
            return Err(BlockIOError::OutOfBounds);
        }
        Ok(())
    }

    #[inline]
    fn lba_and_off(&self, abs_off: u64) -> (Lba, usize) {
        let bs = self.block_size() as u64;
        ((abs_off / bs) as Lba, (abs_off % bs) as usize)
    }

    fn read_block_exact(&mut self, lba: Lba, buf: &mut [u8]) -> BlockIOResult {
        debug_assert_eq!(buf.len(), self.block_size());
        self.blk
            .read_blocks(self.media_id(), lba, buf)
            .map_err(|_| BlockIOError::Other("UEFI read_blocks failed"))
    }

    fn write_block_exact(&mut self, lba: Lba, buf: &[u8]) -> BlockIOResult {
        debug_assert_eq!(buf.len(), self.block_size());
        let media_id = self.media_id();
        self.blk
            .write_blocks(media_id, lba, buf)
            .map_err(|_| BlockIOError::Other("UEFI write_blocks failed"))
    }

    // --- replace fn temp_block_buf(...) by this version ---

    #[inline]
    fn temp_block_buf(&self) -> Result<TempBlockBuf, BlockIOError> {
        let bs = self.block_size();

        // No-alloc: only 512B blocks are supported (stack)
        #[cfg(all(not(feature = "std"), not(feature = "alloc")))]
        {
            if bs <= 512 {
                Ok(TempBlockBuf::Stack512([0u8; 512], bs))
            } else {
                Err(BlockIOError::Unsupported)
            }
        }

        // With alloc/std: prefer boxed fixed arrays, fallback to heap for larger
        #[cfg(any(feature = "std", feature = "alloc"))]
        {
            if bs <= 512 {
                Ok(TempBlockBuf::Stack512(Box::new([0u8; 512]), bs))
            } else if bs <= crate::BLOCK_BUF_SIZE {
                Ok(TempBlockBuf::Stack4096(
                    Box::new([0u8; crate::BLOCK_BUF_SIZE]),
                    bs,
                ))
            } else {
                Ok(TempBlockBuf::Heap(vec![0u8; bs], bs))
            }
        }
    }
}

// No-alloc build: only a small fixed stack buffer (512B)
#[cfg(all(not(feature = "std"), not(feature = "alloc")))]
enum TempBlockBuf {
    Stack512([u8; 512], usize),
}

// Alloc or std: keep small footprint by boxing large arrays
#[cfg(any(feature = "std", feature = "alloc"))]
enum TempBlockBuf {
    Stack512(Box<[u8; 512]>, usize),
    // keep 4KiB for typical block sizes, but boxed to avoid large enum variant
    Stack4096(Box<[u8; crate::BLOCK_BUF_SIZE]>, usize),
    Heap(Vec<u8>, usize),
}

impl TempBlockBuf {
    #[inline]
    fn as_mut(&mut self) -> &mut [u8] {
        match self {
            TempBlockBuf::Stack512(b, n) => &mut b[..*n],
            #[cfg(any(feature = "std", feature = "alloc"))]
            TempBlockBuf::Stack4096(b, n) => &mut b[..*n],
            #[cfg(any(feature = "std", feature = "alloc"))]
            TempBlockBuf::Heap(b, n) => {
                if b.len() != *n {
                    unsafe {
                        b.set_len(*n);
                    }
                }
                &mut b[..]
            }
        }
    }
}

impl BlockIO for UefiBlockIO {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> BlockIOResult {
        let abs_off = self.partition_offset + offset;
        self.check_bounds(abs_off, data.len())?;

        let bs = self.block_size();
        let mut remaining = data;

        // Fast path: aligned full blocks
        if abs_off % bs as u64 == 0 && remaining.len() % bs == 0 {
            let mut lba = (abs_off / bs as u64) as Lba;
            for chunk in remaining.chunks(bs) {
                self.write_block_exact(lba, chunk)?;
                lba += 1;
            }
            return Ok(());
        }

        // Head (unaligned)
        let (mut lba, off_in_blk) = self.lba_and_off(abs_off);
        if off_in_blk != 0 {
            let mut blk = self.temp_block_buf()?;
            let b = blk.as_mut();
            self.read_block_exact(lba, b)?;
            let head = (bs - off_in_blk).min(remaining.len());
            b[off_in_blk..off_in_blk + head].copy_from_slice(&remaining[..head]);
            self.write_block_exact(lba, b)?;
            remaining = &remaining[head..];
            lba += 1;
        }

        // Body (full blocks)
        while remaining.len() >= bs {
            self.write_block_exact(lba, &remaining[..bs])?;
            remaining = &remaining[bs..];
            lba += 1;
        }

        // Tail (partial)
        if !remaining.is_empty() {
            let mut blk = self.temp_block_buf()?;
            let b = blk.as_mut();
            self.read_block_exact(lba, b)?;
            b[..remaining.len()].copy_from_slice(remaining);
            self.write_block_exact(lba, b)?;
        }

        Ok(())
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> BlockIOResult {
        let abs_off = self.partition_offset + offset;
        self.check_bounds(abs_off, buf.len())?;

        let bs = self.block_size();
        let mut remaining = buf;

        // Fast path: aligned full blocks
        if abs_off % bs as u64 == 0 && remaining.len() % bs == 0 {
            let mut lba = (abs_off / bs as u64) as Lba;
            for chunk in remaining.chunks_mut(bs) {
                self.read_block_exact(lba, chunk)?;
                lba += 1;
            }
            return Ok(());
        }

        // Head (unaligned)
        let (mut lba, off_in_blk) = self.lba_and_off(abs_off);
        if off_in_blk != 0 {
            let mut blk = self.temp_block_buf()?;
            let b = blk.as_mut();
            self.read_block_exact(lba, b)?;
            let head = (bs - off_in_blk).min(remaining.len());
            remaining[..head].copy_from_slice(&b[off_in_blk..off_in_blk + head]);
            remaining = &mut remaining[head..];
            lba += 1;
        }

        // Body (full blocks)
        while remaining.len() >= bs {
            self.read_block_exact(lba, &mut remaining[..bs])?;
            remaining = &mut remaining[bs..];
        }

        // Tail (partial)
        if !remaining.is_empty() {
            let mut blk = self.temp_block_buf()?;
            let b = blk.as_mut();
            self.read_block_exact(lba, b)?;
            let n = remaining.len();
            remaining.copy_from_slice(&b[..n]);
        }

        Ok(())
    }

    fn flush(&mut self) -> BlockIOResult {
        self.blk
            .flush_blocks()
            .map_err(|_| BlockIOError::Other("UEFI flush_blocks failed"))
    }

    #[inline]
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    #[inline]
    fn partition_offset(&self) -> u64 {
        self.partition_offset
    }
}
