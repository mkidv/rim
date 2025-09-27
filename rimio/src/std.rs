// SPDX-License-Identifier: MIT

#[cfg(feature = "std")]
use std::io::{Error, Read, Seek, SeekFrom, Write};

#[cfg(feature = "std")]
use crate::BlockIOSetLen;
use crate::{BlockIO, BlockIOError, BlockIOResult};

#[cfg(feature = "std")]
#[derive(Debug)]
pub struct StdBlockIO<'a, T: Read + Write + Seek> {
    io: &'a mut T,
    partition_offset: u64,
}

#[cfg(feature = "std")]
impl<'a, T: Read + Write + Seek> StdBlockIO<'a, T> {
    #[inline]
    pub fn new(io: &'a mut T) -> Self {
        Self {
            io,
            partition_offset: 0,
        }
    }
    
    #[inline]
    pub fn new_with_offset(io: &'a mut T, partition_offset: u64) -> Self {
        Self {
            io,
            partition_offset,
        }
    }
}

#[cfg(feature = "std")]
impl<'a, T: Read + Write + Seek> BlockIO for StdBlockIO<'a, T> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> BlockIOResult {
        let abs_offset = self.partition_offset + offset;
        self.io.seek(SeekFrom::Start(abs_offset))?;
        self.io.write_all(data)?;
        Ok(())
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> BlockIOResult {
        let abs_offset = self.partition_offset + offset;
        self.io.seek(SeekFrom::Start(abs_offset))?;
        self.io.read_exact(buf)?;
        Ok(())
    }

    fn flush(&mut self) -> BlockIOResult {
        self.io.flush()?;
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
}

#[cfg(feature = "std")]
impl<'a> BlockIOSetLen for StdBlockIO<'a, std::fs::File> {
    fn set_len(&mut self, len: u64) -> BlockIOResult {
        self.io.set_len(self.partition_offset + len)?;
        self.flush()?;
        self.io.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

#[cfg(feature = "std")]
impl From<Error> for BlockIOError {
    #[cold]
    #[inline(never)]
    fn from(e: Error) -> Self {
        // Leak the string to produce a 'static str. Acceptable for error mapping.
        let leaked_str: &'static str = Box::leak(e.to_string().into_boxed_str());
        BlockIOError::Other(leaked_str)
    }
}

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use crate::prelude::*;
    use tempfile::tempfile;

    #[test]
    fn test_rw() {
        let mut file = tempfile().unwrap();
        let mut io = StdBlockIO::new(&mut file);
        io.write_at(10, &[1, 2, 3, 4]).unwrap();

        let mut output = [0u8; 4];
        io.read_at(10, &mut output).unwrap();
        assert_eq!(output, [1, 2, 3, 4]);
    }

    #[test]
    fn test_set_len_safe() {
        let mut file = tempfile().unwrap();
        let mut io = StdBlockIO::new(&mut file);

        io.set_len(512).unwrap();
        assert!(io.set_len(u64::MAX).is_err());
    }

    #[test]
    fn test_best_effort_rw_unaligned() {
        let mut file = tempfile().unwrap();
        let mut io = StdBlockIO::new(&mut file);

        let input = [0xAB; 17];
        let mut output = [0u8; 17];

        io.write_block_best_effort(5, &input, 8).unwrap();
        io.read_block_best_effort(5, &mut output, 8).unwrap();

        assert_eq!(input, output);
    }

    #[test]
    fn test_multi_rw() {
        let mut file = tempfile().unwrap();
        let mut io = StdBlockIO::new(&mut file);

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
        let mut file = tempfile().unwrap();
        let mut io = StdBlockIO::new(&mut file);

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
        let mut file = tempfile().unwrap();
        let mut io = StdBlockIO::new(&mut file);

        io.write_at(42, &[0xFF; 8]).unwrap();
        io.zero_fill(42, 8).unwrap();

        let mut buf = [0xAA; 8];
        io.read_at(42, &mut buf).unwrap();

        assert_eq!(buf, [0u8; 8]);
    }
}
