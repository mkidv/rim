// SPDX-License-Identifier: MIT

use crate::{BlockIO, BlockIOError, BlockIOResult, BlockIOSetLen};

/// In-memory implementation of `BlockIO`.
///
/// Useful for tests, RAM-backed FS, virtual disks.
#[derive(Debug)]
pub struct MemBlockIO<'a> {
    buffer: &'a mut [u8],
    partition_offset: u64,
    logical_len: usize,
}

impl<'a> MemBlockIO<'a> {
    /// Creates a new `MemBlockIO` over the given memory buffer.
    pub fn new(buffer: &'a mut [u8]) -> Self {
        let logical_len = buffer.len();

        Self {
            buffer,
            logical_len,
            partition_offset: 0,
        }
    }

    pub fn new_with_offset(buffer: &'a mut [u8], partition_offset: u64) -> Self {
        let logical_len = buffer.len();

        Self {
            buffer,
            logical_len,
            partition_offset,
        }
    }

    fn check_bounds(&self, offset: u64, len: usize) -> BlockIOResult {
        let end = (offset as usize).saturating_add(len);
        if end > self.logical_len {
            return Err(BlockIOError::OutOfBounds);
        }
        Ok(())
    }
}

impl<'a> BlockIO for MemBlockIO<'a> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> BlockIOResult {
        let abs_offset = self.partition_offset + offset;
        self.check_bounds(abs_offset, data.len())?;
        let dst = &mut self.buffer[abs_offset as usize..abs_offset as usize + data.len()];
        dst.copy_from_slice(data);
        Ok(())
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> BlockIOResult {
        let abs_offset = self.partition_offset + offset;
        self.check_bounds(abs_offset, buf.len())?;
        let src = &self.buffer[abs_offset as usize..abs_offset as usize + buf.len()];
        buf.copy_from_slice(src);
        Ok(())
    }

    fn flush(&mut self) -> BlockIOResult {
        Ok(())
    }

    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    fn partition_offset(&self) -> u64 {
        self.partition_offset
    }
}

impl<'a> BlockIOSetLen for MemBlockIO<'a> {
    fn set_len(&mut self, new_len: u64) -> BlockIOResult {
        let new_len = new_len as usize;
        let end = self.partition_offset as usize + new_len;
        if end > self.buffer.len() {
            return Err(BlockIOError::OutOfBounds);
        }
        self.logical_len = new_len;
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
        let mut io = MemBlockIO::new(&mut buf);
        io.write_at(10, &[1, 2, 3, 4]).unwrap();

        let mut output = [0u8; 4];
        io.read_at(10, &mut output).unwrap();
        assert_eq!(output, [1, 2, 3, 4]);
    }

    #[test]
    fn test_set_len_safe() {
        let mut buf = [0u8; 512];
        let mut io = MemBlockIO::new(&mut buf);
        io.set_len(512).unwrap();
        assert!(io.set_len(1024).is_err());
    }

    #[test]
    fn test_best_effort_rw_unaligned() {
        let mut buf = [0u8; 64];
        let mut io = MemBlockIO::new(&mut buf);

        let input = [0xAB; 17];
        let mut output = [0u8; 17];

        io.write_block_best_effort(5, &input, 8).unwrap();
        io.read_block_best_effort(5, &mut output, 8).unwrap();

        assert_eq!(input, output);
    }

    #[test]
    fn test_multi_rw() {
        let mut buf = [0u8; 64];
        let mut io = MemBlockIO::new(&mut buf);

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
        let mut io = MemBlockIO::new(&mut buf);

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
        let mut io = MemBlockIO::new(&mut buf);

        io.zero_fill(10, 8).unwrap();

        let mut output = [0xAA; 8];
        io.read_at(10, &mut output).unwrap();
        assert_eq!(output, [0u8; 8]);
    }
}
