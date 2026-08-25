// SPDX-License-Identifier: MIT

#[cfg(feature = "std")]
use std::io::{Error, Read, Seek, SeekFrom, Write};

#[cfg(feature = "std")]
use crate::RimIOSetLen;
use crate::{RimIO, RimIOError, RimIOResult};

#[cfg(feature = "std")]
#[derive(Debug)]
pub struct StdRimIO<'a, T: Read + Write + Seek> {
    io: &'a mut T,
    partition_offset: u64,
}

#[cfg(feature = "std")]
impl<'a, T: Read + Write + Seek> StdRimIO<'a, T> {
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
impl<'a, T: Read + Write + Seek> RimIO for StdRimIO<'a, T> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let abs_offset = self.partition_offset + offset;
        self.io.seek(SeekFrom::Start(abs_offset))?;
        self.io.write_all(data)?;
        Ok(())
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let abs_offset = self.partition_offset + offset;
        self.io.seek(SeekFrom::Start(abs_offset))?;
        self.io.read_exact(buf)?;
        Ok(())
    }

    fn flush(&mut self) -> RimIOResult {
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

    fn total_size(&mut self) -> RimIOResult<u64> {
        let current = self.io.stream_position()?;
        let end = self.io.seek(SeekFrom::End(0))?;
        self.io.seek(SeekFrom::Start(current))?;
        Ok(end.saturating_sub(self.partition_offset))
    }
}

/// A specialized `RimIO` implementation for `std::fs::File`.
///
/// Uses OS-specific offset I/O (pread/pwrite) to avoid seek overhead and mutable state issues.
/// Significantly faster for random access than `StdRimIO` wrapping a File.
#[cfg(feature = "std")]
#[derive(Debug)]
pub struct FileRimIO {
    file: std::fs::File,
    partition_offset: u64,
}

#[cfg(feature = "std")]
impl FileRimIO {
    pub fn new(file: std::fs::File) -> Self {
        Self {
            file,
            partition_offset: 0,
        }
    }
}

#[cfg(feature = "std")]
impl RimIO for FileRimIO {
    #[cfg(target_family = "unix")]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        use std::os::unix::fs::FileExt;
        let abs_offset = self.partition_offset + offset;
        self.file.write_all_at(data, abs_offset)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        use std::os::windows::fs::FileExt;
        let abs_offset = self.partition_offset + offset;
        self.file.seek_write(data, abs_offset)?;
        Ok(())
    }

    #[cfg(not(any(target_family = "unix", target_os = "windows")))]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let abs_offset = self.partition_offset + offset;
        // fallback to seek if not supported (rare for std)
        // We need a ref to file, but seek needs mut. File needs mut for seek?
        // std::fs::File seek takes &mut self.
        // So we are good.
        use std::io::{Seek, Serializer};
        self.file.seek(SeekFrom::Start(abs_offset))?;
        self.file.write_all(data)?;
        Ok(())
    }

    #[cfg(target_family = "unix")]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        use std::os::unix::fs::FileExt;
        let abs_offset = self.partition_offset + offset;
        self.file.read_exact_at(buf, abs_offset)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        use std::os::windows::fs::FileExt;
        let abs_offset = self.partition_offset + offset;
        let mut read = 0;
        while read < buf.len() {
            let n = self
                .file
                .seek_read(&mut buf[read..], abs_offset + read as u64)?;
            if n == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
            }
            read += n;
        }
        Ok(())
    }

    #[cfg(not(any(target_family = "unix", target_os = "windows")))]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let abs_offset = self.partition_offset + offset;
        self.file.seek(SeekFrom::Start(abs_offset))?;
        self.file.read_exact(buf)?;
        Ok(())
    }

    fn flush(&mut self) -> RimIOResult {
        self.file.flush()?;
        Ok(())
    }

    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    fn partition_offset(&self) -> u64 {
        self.partition_offset
    }

    fn total_size(&mut self) -> RimIOResult<u64> {
        let len = self.file.metadata()?.len();
        Ok(len.saturating_sub(self.partition_offset))
    }
}

#[cfg(feature = "std")]
impl RimIOSetLen for FileRimIO {
    fn set_len(&mut self, len: u64) -> RimIOResult {
        let abs_len = self.partition_offset + len;
        self.file.set_len(abs_len)?;
        Ok(())
    }
}

#[cfg(feature = "std")]
impl<'a> RimIOSetLen for StdRimIO<'a, std::fs::File> {
    fn set_len(&mut self, len: u64) -> RimIOResult {
        self.io.set_len(self.partition_offset + len)?;
        self.flush()?;
        self.io.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}

#[cfg(feature = "std")]
impl From<Error> for RimIOError {
    #[cold]
    #[inline(never)]
    fn from(e: Error) -> Self {
        // Leak the string to produce a 'static str. Acceptable for error mapping.
        let leaked_str: &'static str = Box::leak(e.to_string().into_boxed_str());
        RimIOError::Other(leaked_str)
    }
}

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use crate::prelude::*;
    use crate::test_suite::*;
    use tempfile::tempfile;

    #[test]
    fn test_std_rimio_suite() {
        {
            let mut file = tempfile().unwrap();
            let mut io = StdRimIO::new(&mut file);
            check_basic_rw(&mut io);
        }
        {
            let mut file = tempfile().unwrap();
            let mut io = StdRimIO::new(&mut file);
            check_rw_at_offset(&mut io);
        }
        {
            let mut file = tempfile().unwrap();
            let mut io = StdRimIO::new(&mut file);
            check_zero_fill(&mut io);
        }
        {
            let mut file = tempfile().unwrap();
            let mut io = StdRimIO::new(&mut file);
            check_bounds(&mut io, 0, true);
        }
    }

    #[test]
    fn test_std_rimio_set_len() {
        let mut file = tempfile().unwrap();
        let mut io = StdRimIO::new(&mut file);
        check_set_len(&mut io);
    }

    #[test]
    fn test_file_rimio_suite() {
        {
            let file = tempfile().unwrap();
            let mut io = FileRimIO::new(file);
            check_basic_rw(&mut io);
        }
        {
            let file = tempfile().unwrap();
            let mut io = FileRimIO::new(file);
            check_rw_at_offset(&mut io);
        }
        {
            let file = tempfile().unwrap();
            let mut io = FileRimIO::new(file);
            check_zero_fill(&mut io);
        }
        {
            let file = tempfile().unwrap();
            let mut io = FileRimIO::new(file);
            check_bounds(&mut io, 0, true);
        }
    }

    #[test]
    fn test_file_rimio_set_len() {
        let file = tempfile().unwrap();
        let mut io = FileRimIO::new(file);
        check_set_len(&mut io);
    }

    #[test]
    fn test_set_len_max() {
        let mut file = tempfile().unwrap();
        let mut io = StdRimIO::new(&mut file);

        io.set_len(512).unwrap();
        assert!(io.set_len(u64::MAX).is_err());
    }

    #[test]
    fn test_best_effort_rw_unaligned() {
        let mut file = tempfile().unwrap();
        let mut io = StdRimIO::new(&mut file);

        let input = [0xAB; 17];
        let mut output = [0u8; 17];

        io.write_block_best_effort(5, &input, 8).unwrap();
        io.read_block_best_effort(5, &mut output, 8).unwrap();

        assert_eq!(input, output);
    }

    #[test]
    fn test_multi_rw() {
        let mut file = tempfile().unwrap();
        let mut io = StdRimIO::new(&mut file);

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
        let mut io = StdRimIO::new(&mut file);

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
}
