use crate::{RimIO, RimIOError, RimIOResult, RimIOSetLen, RimRead, RimWrite};
use memmap2::MmapMut;
use std::fs::File;
use std::io;

/// Memory-mapped implementation of `RimIO` using `memmap2`.
///
/// This provides extremely fast random access (zero syscalls for read/write)
/// by mapping the file directly into process memory.
///
/// # Platform Support
/// Requires `mmap` feature.
#[derive(Debug)]
pub struct MmapRimIO {
    mmap: Option<MmapMut>, // Option to allow re-mapping (drop before map)
    file: File,
    partition_offset: u64,
    len: u64,
}

impl MmapRimIO {
    /// Creates a new memory mapped IO from a standard file.
    pub fn new(file: File) -> io::Result<Self> {
        let len = file.metadata()?.len();
        // Handle empty files which cannot be mmapped
        if len == 0 {
            // We return same error as before or handle empty?
            // For now, allow len=0 but mmap might be None or dummy?
            // memmap2 with len=0 fails usually.
        }

        let mmap = if len > 0 {
            // SAFETY: We own the file so we can safely map it.
            Some(unsafe { MmapMut::map_mut(&file)? })
        } else {
            None
        };

        Ok(Self {
            mmap,
            file,
            partition_offset: 0,
            len,
        })
    }

    /// Creates a new memory mapped IO with an offset.
    pub fn new_with_offset(file: File, partition_offset: u64) -> io::Result<Self> {
        let mut me = Self::new(file)?;
        me.partition_offset = partition_offset;
        Ok(me)
    }

    #[inline(always)]
    fn check_bounds(&self, abs_off: u64, len: usize) -> RimIOResult {
        let end = abs_off
            .checked_add(len as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.len {
            return Err(RimIOError::OutOfBounds);
        }
        Ok(())
    }
}

impl RimRead for MmapRimIO {
    #[inline(always)]
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let abs_off = self.partition_offset + offset;
        self.check_bounds(abs_off, buf.len())?;

        let mmap = self.mmap.as_ref().ok_or(RimIOError::Other("Empty mmap"))?;
        let src = &mmap[abs_off as usize..abs_off as usize + buf.len()];
        buf.copy_from_slice(src);
        Ok(())
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.len.saturating_sub(self.partition_offset))
    }
}

impl RimWrite for MmapRimIO {
    #[inline(always)]
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let abs_off = self.partition_offset + offset;
        self.check_bounds(abs_off, data.len())?;

        let mmap = self.mmap.as_mut().ok_or(RimIOError::Other("Empty mmap"))?;
        let dst = &mut mmap[abs_off as usize..abs_off as usize + data.len()];
        dst.copy_from_slice(data);
        Ok(())
    }

    fn flush(&mut self) -> RimIOResult {
        if let Some(mmap) = self.mmap.as_mut() {
            mmap.flush()
                .map_err(|e| RimIOError::Other(Box::leak(e.to_string().into_boxed_str())))
        } else {
            Ok(())
        }
    }
}

impl RimIO for MmapRimIO {
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

impl RimIOSetLen for MmapRimIO {
    fn set_len(&mut self, new_len: u64) -> RimIOResult {
        // 1. Drop existing map to allow resize (esp on Windows)
        // We use .take() to ensure it is dropped before file operation.
        self.flush()?;
        self.mmap = None;

        // 2. Resize underlying file
        let abs_new_len = self.partition_offset + new_len;
        self.file
            .set_len(abs_new_len)
            .map_err(|_| RimIOError::Other("Failed to set file length"))?;

        // 3. Remap
        if abs_new_len > 0 {
            let mmap = unsafe {
                MmapMut::map_mut(&self.file)
                    .map_err(|_| RimIOError::Other("Failed to remap file"))?
            };
            self.mmap = Some(mmap);
        }

        self.len = abs_new_len;
        Ok(())
    }
}

#[cfg(all(test, feature = "std", feature = "mmap"))]
mod tests {
    use super::*;
    use crate::test_suite::*;
    use std::io::Write;
    use tempfile::tempfile;

    #[test]
    fn test_mmap_rimio_suite() {
        let mut file = tempfile().unwrap();
        file.set_len(1024).unwrap();
        // Zero it to avoid random trash if tempfile recycles
        file.write_all(&[0u8; 1024]).unwrap();

        let mut io = MmapRimIO::new(file).unwrap();

        check_basic_rw(&mut io);
        check_rw_at_offset(&mut io);
        check_zero_fill(&mut io);
        check_bounds(&mut io, 1024, false);
    }

    #[test]
    fn test_mmap_rimio_set_len() {
        let file = tempfile().unwrap();
        file.set_len(512).unwrap();
        let mut io = MmapRimIO::new(file).unwrap();
        check_set_len(&mut io);
    }
}
