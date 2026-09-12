// SPDX-License-Identifier: MIT

//! In-memory buffer implementations of RimIO (MemRimIO and BoundedRimIO).

use crate::{RimIO, RimIOError, RimIOResult, RimIOSetLen, RimRead, RimWrite, checked_add_offset};

/// Read-only in-memory slice implementation of `RimRead`.
#[derive(Debug, Clone, Copy)]
pub struct SliceRimIO<'a> {
    buffer: &'a [u8],
}

impl<'a> SliceRimIO<'a> {
    #[inline]
    pub const fn new(buffer: &'a [u8]) -> Self {
        Self { buffer }
    }
}

impl<'a> RimRead for SliceRimIO<'a> {
    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let start = usize::try_from(offset).map_err(|_| RimIOError::OutOfBounds)?;
        let end = start
            .checked_add(buf.len())
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.buffer.len() {
            return Err(RimIOError::OutOfBounds);
        }
        buf.copy_from_slice(&self.buffer[start..end]);
        Ok(())
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.buffer.len() as u64)
    }
}

/// Read-only heap-allocated vector implementation of `RimRead`.
#[cfg(feature = "alloc")]
#[derive(Debug, Clone)]
pub struct VecRimIO {
    buffer: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl VecRimIO {
    #[inline]
    pub const fn new(buffer: alloc::vec::Vec<u8>) -> Self {
        Self { buffer }
    }

    #[inline]
    pub fn into_vec(self) -> alloc::vec::Vec<u8> {
        self.buffer
    }
}

#[cfg(feature = "alloc")]
impl RimRead for VecRimIO {
    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let start = usize::try_from(offset).map_err(|_| RimIOError::OutOfBounds)?;
        let end = start
            .checked_add(buf.len())
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.buffer.len() {
            return Err(RimIOError::OutOfBounds);
        }
        buf.copy_from_slice(&self.buffer[start..end]);
        Ok(())
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.buffer.len() as u64)
    }
}

/// A bounded window over any `RimRead`, `RimWrite`, or `RimIO` source.
///
/// I/O operations are offset by `offset` and clamped to `size`.
#[derive(Debug, Clone)]
pub struct BoundedRimIO<T> {
    source: T,
    offset: u64,
    size: u64,
}

impl<T> BoundedRimIO<T> {
    #[inline]
    pub const fn new(source: T, offset: u64, size: u64) -> Self {
        Self {
            source,
            offset,
            size,
        }
    }

    #[inline]
    pub fn offset(&self) -> u64 {
        self.offset
    }

    #[inline]
    pub fn size(&self) -> u64 {
        self.size
    }

    #[inline]
    pub fn source(&self) -> &T {
        &self.source
    }

    #[inline]
    pub fn source_mut(&mut self) -> &mut T {
        &mut self.source
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.source
    }
}

impl<T: RimRead> RimRead for BoundedRimIO<T> {
    #[inline]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let end = offset
            .checked_add(buf.len() as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.size {
            return Err(RimIOError::OutOfBounds);
        }
        let abs_offset = self
            .offset
            .checked_add(offset)
            .ok_or(RimIOError::OutOfBounds)?;
        self.source.read_at(abs_offset, buf)
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.size)
    }
}

impl<T: RimWrite> RimWrite for BoundedRimIO<T> {
    #[inline]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let end = offset
            .checked_add(data.len() as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.size {
            return Err(RimIOError::OutOfBounds);
        }
        let abs_offset = self
            .offset
            .checked_add(offset)
            .ok_or(RimIOError::OutOfBounds)?;
        self.source.write_at(abs_offset, data)
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        self.source.flush()
    }
}

impl<T: RimIO> RimIO for BoundedRimIO<T> {
    #[inline]
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.offset = partition_offset;
        partition_offset
    }

    #[inline]
    fn partition_offset(&self) -> u64 {
        self.offset
    }
}

impl<T: RimIOSetLen> RimIOSetLen for BoundedRimIO<T> {
    fn set_len(&mut self, new_len: u64) -> RimIOResult {
        if new_len > self.size {
            return Err(RimIOError::OutOfBounds);
        }
        self.size = new_len;
        Ok(())
    }
}

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
        let max = self.logical_len as u64;
        if end > max {
            return Err(RimIOError::OutOfBounds);
        }
        Ok(())
    }
}

impl<'a> RimRead for MemRimIO<'a> {
    #[inline(always)]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let abs_offset = checked_add_offset(self.partition_offset, offset)?;
        self.check_bounds(abs_offset, buf.len())?;
        let src = &self.buffer[abs_offset as usize..abs_offset as usize + buf.len()];
        buf.copy_from_slice(src);
        Ok(())
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok((self.logical_len as u64).saturating_sub(self.partition_offset))
    }
}

impl<'a> RimWrite for MemRimIO<'a> {
    #[inline(always)]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let abs_offset = checked_add_offset(self.partition_offset, offset)?;
        self.check_bounds(abs_offset, data.len())?;
        let dst = &mut self.buffer[abs_offset as usize..abs_offset as usize + data.len()];
        dst.copy_from_slice(data);
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        Ok(())
    }
}

impl<'a> RimIO for MemRimIO<'a> {
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

impl<'a> RimIOSetLen for MemRimIO<'a> {
    fn set_len(&mut self, new_len: u64) -> RimIOResult {
        let end = self
            .partition_offset
            .checked_add(new_len)
            .ok_or(RimIOError::OutOfBounds)? as usize;
        if end > self.buffer.len() {
            return Err(RimIOError::OutOfBounds);
        }
        self.logical_len = end;
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod test {
    use super::*;
    use crate::prelude::*;
    use crate::test_suite::*;

    #[test]
    fn test_mem_rimio_partition_view_invariants() {
        let mut buf = [0u8; 1000];
        let mut io = MemRimIO::new(&mut buf);
        io.set_offset(100);

        // Invariant 1: total_size() of the initial view (1000 - 100 = 900)
        assert_eq!(io.total_size().unwrap(), 900);

        // Invariant 2: bounds checking within the view (0..900)
        assert!(io.write_at(899, &[0xAA]).is_ok());
        assert!(io.write_at(900, &[0xAA]).is_err());

        // Invariant 3: set_len(500) resizes the view to 500 bytes
        io.set_len(500).unwrap();
        assert_eq!(io.total_size().unwrap(), 500);

        // Invariant 4: bounds checking within the resized view (0..500)
        assert!(io.write_at(499, &[0xBB]).is_ok());
        assert!(io.write_at(500, &[0xBB]).is_err());
    }

    #[test]
    fn test_mem_rimio_rejects_offset_overflow() {
        let mut buf = [0u8; 16];
        let mut io = MemRimIO::new_with_offset(&mut buf, u64::MAX);
        let mut out = [0u8; 1];

        assert!(io.read_at(1, &mut out).is_err());
        assert!(io.write_at(1, &[0xAA]).is_err());
    }

    #[test]
    fn test_mem_rimio_suite() {
        let mut buf = [0u8; 4096];
        let mut io = MemRimIO::new(&mut buf);

        check_basic_rw(&mut io);
        check_rw_at_offset(&mut io);
        check_zero_fill(&mut io);

        // MemRimIO capacity is 4096.
        // Initial logical len is 4096.
        check_bounds(&mut io, 4096, false);
    }

    #[test]
    fn test_mem_rimio_set_len() {
        let mut buf = [0u8; 4096];
        let mut io = MemRimIO::new(&mut buf);
        // MemRimIO starts with len=4096.
        // check_set_len expects to resizing.
        check_set_len(&mut io);
    }

    #[test]
    fn test_mem_rimio_best_effort() {
        let mut buf = [0u8; 64];
        let mut io = MemRimIO::new(&mut buf);

        let input = [0xAB; 17];
        let mut output = [0u8; 17];

        io.write_block_best_effort(5, &input, 8).unwrap();
        io.read_block_best_effort(5, &mut output, 8).unwrap();

        assert_eq!(input, output);
    }

    #[test]
    fn test_mem_rimio_multi_rw() {
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
    fn test_mem_rimio_streamed() {
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
    fn test_bounded_rimio_read_write() {
        let mut disk = [0u8; 1000];
        let mut mem_io = MemRimIO::new(&mut disk);

        let mut bounded = BoundedRimIO::new(&mut mem_io, 100, 200);
        assert_eq!(bounded.total_size().unwrap(), 200);
        assert_eq!(bounded.offset(), 100);
        assert_eq!(bounded.size(), 200);

        assert!(bounded.write_at(0, b"PARTITION_START").is_ok());
        assert!(bounded.write_at(180, b"END").is_ok());

        // Out-of-bounds checks
        assert!(bounded.write_at(199, b"AB").is_err());
        assert!(bounded.write_at(200, b"X").is_err());

        let mut read_buf = [0u8; 15];
        assert!(bounded.read_at(0, &mut read_buf).is_ok());
        assert_eq!(&read_buf, b"PARTITION_START");

        assert_eq!(&disk[100..115], b"PARTITION_START");
        assert_eq!(&disk[280..283], b"END");
        assert_eq!(disk[99], 0);
        assert_eq!(disk[300], 0);
    }
}
