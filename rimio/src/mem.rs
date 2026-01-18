// SPDX-License-Identifier: MIT

use crate::{RimIO, RimIOError, RimIOResult, RimIOSetLen};

/// In-memory implementation of `RimIO`.
///
/// Useful for tests, RAM-backed FS, virtual disks.
#[derive(Debug)]
pub struct MemRimIO<'a> {
    buffer: &'a mut [u8],
    partition_offset: u64,
    logical_len: usize,
}

impl<'a> MemRimIO<'a> {
    #[inline]
    pub fn new(buffer: &'a mut [u8]) -> Self {
        let logical_len = buffer.len();

        Self {
            buffer,
            logical_len,
            partition_offset: 0,
        }
    }

    #[inline]
    pub fn new_with_offset(buffer: &'a mut [u8], partition_offset: u64) -> Self {
        let logical_len = buffer.len();

        Self {
            buffer,
            logical_len,
            partition_offset,
        }
    }

    #[inline]
    fn check_bounds(&self, abs_off: u64, len: usize) -> RimIOResult {
        let end = abs_off
            .checked_add(len as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        let max = self
            .partition_offset
            .checked_add(self.logical_len as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        if end > max {
            return Err(RimIOError::OutOfBounds);
        }
        Ok(())
    }
}

impl<'a> RimIO for MemRimIO<'a> {
    #[inline(always)]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let abs_offset = self.partition_offset + offset;
        self.check_bounds(abs_offset, data.len())?;
        let dst = &mut self.buffer[abs_offset as usize..abs_offset as usize + data.len()];
        dst.copy_from_slice(data);
        Ok(())
    }

    #[inline(always)]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let abs_offset = self.partition_offset + offset;
        self.check_bounds(abs_offset, buf.len())?;
        let src = &self.buffer[abs_offset as usize..abs_offset as usize + buf.len()];
        buf.copy_from_slice(src);
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        Ok(())
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

    /// Optimized single-copy implementation.
    /// Reads directly from `src` into the internal buffer segment.
    fn copy_from(
        &mut self,
        src: &mut dyn RimIO,
        src_offset: u64,
        dest_offset: u64,
        len: u64,
    ) -> RimIOResult {
        let abs_offset = self.partition_offset + dest_offset;
        let len_usize = len as usize;
        self.check_bounds(abs_offset, len_usize)?;

        let dst = &mut self.buffer[abs_offset as usize..abs_offset as usize + len_usize];
        src.read_at(src_offset, dst)?;
        Ok(())
    }
}

impl<'a> RimIOSetLen for MemRimIO<'a> {
    fn set_len(&mut self, new_len: u64) -> RimIOResult {
        let end = self
            .partition_offset
            .checked_add(new_len)
            .ok_or(RimIOError::OutOfBounds)? as usize;
        if end > self.buffer.len() {
            return Err(RimIOError::OutOfBounds);
        }
        self.logical_len = new_len as usize;
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn test_rw() {
        let mut buf = [0u8; 256];
        let mut io = MemRimIO::new(&mut buf);
        io.write_at(10, &[1, 2, 3, 4]).unwrap();

        let mut output = [0u8; 4];
        io.read_at(10, &mut output).unwrap();
        assert_eq!(output, [1, 2, 3, 4]);
    }

    #[test]
    fn test_set_len_safe() {
        let mut buf = [0u8; 512];
        let mut io = MemRimIO::new(&mut buf);
        io.set_len(512).unwrap();
        assert!(io.set_len(1024).is_err());
    }

    #[test]
    fn test_best_effort_rw_unaligned() {
        let mut buf = [0u8; 64];
        let mut io = MemRimIO::new(&mut buf);

        let input = [0xAB; 17];
        let mut output = [0u8; 17];

        io.write_block_best_effort(5, &input, 8).unwrap();
        io.read_block_best_effort(5, &mut output, 8).unwrap();

        assert_eq!(input, output);
    }

    #[test]
    fn test_multi_rw() {
        let mut buf = [0u8; 64];
        let mut io = MemRimIO::new(&mut buf);

        let cluster_size = 8;
        let clusters = 4;
        let input = [0xCD; 32];
        let mut output = [0u8; 32];

        let offsets: Vec<u64> = (0..clusters).map(|i| i * cluster_size as u64).collect();

        io.write_multi_at(&offsets, cluster_size, &input).unwrap();
        io.read_multi_at(&offsets, cluster_size, &mut output)
            .unwrap();

        assert_eq!(input, output);
    }

    #[test]
    fn test_chunks_streamed_rw() {
        let mut buf = [0u8; 1024];
        let mut io = MemRimIO::new(&mut buf);

        io.write_chunks_streamed::<4, _>(0, 10, 5, |i| (i as u32).to_le_bytes())
            .unwrap();

        let mut values = [0u32; 10];
        io.read_chunks_streamed::<4, _>(0, 10, 5, |i, bytes| {
            values[i] = u32::from_le_bytes(*bytes);
        })
        .unwrap();

        for (i, v) in values.iter().enumerate() {
            assert_eq!(*v, i as u32);
        }
    }

    #[test]
    fn test_zero_fill() {
        let mut buf = [0xFF; 64];
        let mut io = MemRimIO::new(&mut buf);

        io.zero_fill(10, 8).unwrap();

        let mut output = [0xAA; 8];
        io.read_at(10, &mut output).unwrap();
        assert_eq!(output, [0u8; 8]);
    }
}
