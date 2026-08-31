#[cfg(feature = "alloc")]
use alloc::{boxed::Box, collections::BTreeMap};

#[cfg(feature = "alloc")]
use crate::{RimIO, RimIOError, RimIOResult, RimIOSetLen, RimRead, RimWrite};

/// Default allocation granularity for [`SparseRimIO`].
#[cfg(feature = "alloc")]
pub const DEFAULT_SPARSE_PAGE_SIZE: usize = 4096;

pub type SparseRimIO = SparseRimIOImpl<DEFAULT_SPARSE_PAGE_SIZE>;

pub type PagedSparseRimIO<const PAGE_SIZE: usize> = SparseRimIOImpl<PAGE_SIZE>;

/// Sparse in-memory implementation of [`RimIO`].
///
/// Represents a potentially very large logical storage while allocating
/// memory only for pages containing non-zero data.
///
/// Unallocated regions read back as zeroes.
///
/// This is useful for:
/// - large virtual disks,
/// - image-generation dry runs,
/// - filesystem/GPT geometry tests,
/// - fuzzing large address spaces.
///
/// `PAGE_SIZE` controls the allocation granularity. 4 KiB is a sensible
/// default for disk-image workloads.
#[cfg(feature = "alloc")]
#[derive(Debug, Clone)]
pub struct SparseRimIOImpl<const PAGE_SIZE: usize = DEFAULT_SPARSE_PAGE_SIZE> {
    pages: BTreeMap<u64, Box<[u8]>>,
    partition_offset: u64,
    logical_len: u64,
}

#[cfg(feature = "alloc")]
impl<const PAGE_SIZE: usize> SparseRimIOImpl<PAGE_SIZE> {
    /// Create an empty sparse storage with the given logical size.
    ///
    /// No backing pages are allocated by this operation.
    pub fn new(logical_len: u64) -> Self {
        assert!(PAGE_SIZE > 0, "SparseRimIO page size must be non-zero");

        Self {
            pages: BTreeMap::new(),
            partition_offset: 0,
            logical_len,
        }
    }

    /// Create sparse storage with an initial partition offset.
    pub fn new_with_offset(logical_len: u64, partition_offset: u64) -> Self {
        assert!(PAGE_SIZE > 0, "SparseRimIO page size must be non-zero");

        Self {
            pages: BTreeMap::new(),
            partition_offset,
            logical_len,
        }
    }

    /// Absolute logical length of the backing storage.
    #[inline]
    pub const fn logical_len(&self) -> u64 {
        self.logical_len
    }

    /// Number of currently allocated sparse pages.
    #[inline]
    pub fn allocated_pages(&self) -> usize {
        self.pages.len()
    }

    /// Approximate payload bytes currently resident in sparse pages.
    ///
    /// This does not include `BTreeMap` / allocation metadata overhead.
    #[inline]
    pub fn allocated_bytes(&self) -> u64 {
        (self.pages.len() as u64).saturating_mul(PAGE_SIZE as u64)
    }

    /// Drop all allocated pages while preserving the logical size.
    ///
    /// The whole storage subsequently reads as zeroes.
    #[inline]
    pub fn clear(&mut self) {
        self.pages.clear();
    }

    #[inline]
    fn absolute_offset(&self, offset: u64) -> RimIOResult<u64> {
        self.partition_offset
            .checked_add(offset)
            .ok_or(RimIOError::OutOfBounds)
    }

    #[inline]
    fn check_bounds(&self, absolute_offset: u64, len: usize) -> RimIOResult {
        let len = u64::try_from(len).map_err(|_| RimIOError::OutOfBounds)?;

        let end = absolute_offset
            .checked_add(len)
            .ok_or(RimIOError::OutOfBounds)?;

        if end > self.logical_len {
            return Err(RimIOError::OutOfBounds);
        }

        Ok(())
    }

    #[inline]
    fn page_size_u64() -> u64 {
        PAGE_SIZE as u64
    }

    #[inline]
    fn new_page() -> Box<[u8]> {
        alloc::vec![0u8; PAGE_SIZE].into_boxed_slice()
    }
}

#[cfg(feature = "alloc")]
impl<const PAGE_SIZE: usize> RimRead for SparseRimIOImpl<PAGE_SIZE> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
        let absolute_offset = self.absolute_offset(offset)?;
        self.check_bounds(absolute_offset, buf.len())?;

        // Sparse holes read as zero.
        buf.fill(0);

        let page_size = Self::page_size_u64();
        let mut dst_pos = 0usize;

        while dst_pos < buf.len() {
            let absolute = absolute_offset + dst_pos as u64;
            let page_index = absolute / page_size;
            let page_offset = (absolute % page_size) as usize;

            let chunk_len = (PAGE_SIZE - page_offset).min(buf.len() - dst_pos);

            if let Some(page) = self.pages.get(&page_index) {
                buf[dst_pos..dst_pos + chunk_len]
                    .copy_from_slice(&page[page_offset..page_offset + chunk_len]);
            }

            dst_pos += chunk_len;
        }

        Ok(())
    }

    #[inline]
    fn total_size(&mut self) -> RimIOResult<u64> {
        Ok(self.logical_len.saturating_sub(self.partition_offset))
    }
}

#[cfg(feature = "alloc")]
impl<const PAGE_SIZE: usize> RimWrite for SparseRimIOImpl<PAGE_SIZE> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult {
        let absolute_offset = self.absolute_offset(offset)?;
        self.check_bounds(absolute_offset, data.len())?;

        let page_size = Self::page_size_u64();
        let mut src_pos = 0usize;

        while src_pos < data.len() {
            let absolute = absolute_offset + src_pos as u64;
            let page_index = absolute / page_size;
            let page_offset = (absolute % page_size) as usize;

            let chunk_len = (PAGE_SIZE - page_offset).min(data.len() - src_pos);
            let chunk = &data[src_pos..src_pos + chunk_len];

            let is_zero = chunk.iter().all(|&byte| byte == 0);

            if is_zero {
                // A full zero page simply becomes a hole.
                if page_offset == 0 && chunk_len == PAGE_SIZE {
                    self.pages.remove(&page_index);
                } else {
                    let remove_page = if let Some(page) = self.pages.get_mut(&page_index) {
                        page[page_offset..page_offset + chunk_len].fill(0);
                        page.iter().all(|&byte| byte == 0)
                    } else {
                        false
                    };

                    if remove_page {
                        self.pages.remove(&page_index);
                    }
                }
            } else {
                let page = self.pages.entry(page_index).or_insert_with(Self::new_page);

                page[page_offset..page_offset + chunk_len].copy_from_slice(chunk);
            }

            src_pos += chunk_len;
        }

        Ok(())
    }

    fn zero_at(&mut self, offset: u64, len: u64) -> RimIOResult {
        if len == 0 {
            return Ok(());
        }

        let start = self
            .partition_offset
            .checked_add(offset)
            .ok_or(RimIOError::OutOfBounds)?;

        let end = start.checked_add(len).ok_or(RimIOError::OutOfBounds)?;

        if end > self.logical_len {
            return Err(RimIOError::OutOfBounds);
        }

        let page_size = PAGE_SIZE as u64;
        let first_page = start / page_size;
        let last_page = (end - 1) / page_size;

        // Avoid mutating BTreeMap while iterating its range.
        let affected: alloc::vec::Vec<u64> = self
            .pages
            .range(first_page..=last_page)
            .map(|(&index, _)| index)
            .collect();

        for page_index in affected {
            let page_start = page_index * page_size;
            let page_end = page_start + page_size;

            let zero_start = start.max(page_start);
            let zero_end = end.min(page_end);

            let local_start = (zero_start - page_start) as usize;
            let local_end = (zero_end - page_start) as usize;

            if local_start == 0 && local_end == PAGE_SIZE {
                self.pages.remove(&page_index);
                continue;
            }

            let remove = if let Some(page) = self.pages.get_mut(&page_index) {
                page[local_start..local_end].fill(0);
                page.iter().all(|&byte| byte == 0)
            } else {
                false
            };

            if remove {
                self.pages.remove(&page_index);
            }
        }

        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> RimIOResult {
        Ok(())
    }
}

#[cfg(feature = "alloc")]
impl<const PAGE_SIZE: usize> RimIO for SparseRimIOImpl<PAGE_SIZE> {
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

#[cfg(feature = "alloc")]
impl<const PAGE_SIZE: usize> RimIOSetLen for SparseRimIOImpl<PAGE_SIZE> {
    fn set_len(&mut self, new_len: u64) -> RimIOResult {
        let new_absolute_len = self
            .partition_offset
            .checked_add(new_len)
            .ok_or(RimIOError::OutOfBounds)?;

        if new_absolute_len < self.logical_len {
            let page_size = Self::page_size_u64();

            let boundary_page = new_absolute_len / page_size;
            let boundary_offset = (new_absolute_len % page_size) as usize;

            // Everything strictly after the final partially retained page
            // can be discarded in one operation.
            let first_dead_page = if boundary_offset == 0 {
                boundary_page
            } else {
                boundary_page + 1
            };

            drop(self.pages.split_off(&first_dead_page));

            // If the new end sits inside a page, bytes beyond the logical
            // end must become zero. Otherwise shrinking and then extending
            // would resurrect old data.
            if boundary_offset != 0 {
                let remove_page = if let Some(page) = self.pages.get_mut(&boundary_page) {
                    page[boundary_offset..].fill(0);
                    page.iter().all(|&byte| byte == 0)
                } else {
                    false
                };

                if remove_page {
                    self.pages.remove(&boundary_page);
                }
            }
        }

        // Extending is free: new space is implicitly sparse/zero-filled.
        self.logical_len = new_absolute_len;

        Ok(())
    }
}

#[test]
fn test_sparse_rimio_large_logical_disk() {
    const TIB: u64 = 1024 * 1024 * 1024 * 1024;

    let mut io = SparseRimIOImpl::<4096>::new(TIB);

    assert_eq!(io.total_size().unwrap(), TIB);
    assert_eq!(io.allocated_pages(), 0);

    let data = [0xAB; 512];
    io.write_at(TIB - 512, &data).unwrap();

    assert_eq!(io.allocated_pages(), 1);

    let mut out = [0u8; 512];
    io.read_at(TIB - 512, &mut out).unwrap();

    assert_eq!(out, data);
}

#[test]
fn test_sparse_rimio_holes_read_zero() {
    let mut io = SparseRimIOImpl::<4096>::new(1024 * 1024);

    io.write_at(128 * 1024, b"RIM").unwrap();

    let mut hole = [0xFF; 128];
    io.read_at(512 * 1024, &mut hole).unwrap();

    assert!(hole.iter().all(|&b| b == 0));
}

#[test]
fn test_sparse_rimio_cross_page_write() {
    let mut io = SparseRimIOImpl::<4096>::new(16 * 1024);

    let data = [0xCD; 32];
    io.write_at(4096 - 16, &data).unwrap();

    assert_eq!(io.allocated_pages(), 2);

    let mut out = [0u8; 32];
    io.read_at(4096 - 16, &mut out).unwrap();

    assert_eq!(out, data);
}

#[test]
fn test_sparse_rimio_zero_write_reclaims_page() {
    let mut io = SparseRimIOImpl::<4096>::new(8192);

    io.write_at(0, &[0xAA; 4096]).unwrap();
    assert_eq!(io.allocated_pages(), 1);

    io.write_at(0, &[0u8; 4096]).unwrap();
    assert_eq!(io.allocated_pages(), 0);
}

#[test]
fn test_sparse_rimio_shrink_does_not_resurrect_data() {
    let mut io = SparseRimIOImpl::<4096>::new(8192);

    io.write_at(5000, b"SECRET").unwrap();

    io.set_len(4096).unwrap();
    io.set_len(8192).unwrap();

    let mut out = [0xFF; 6];
    io.read_at(5000, &mut out).unwrap();

    assert_eq!(out, [0; 6]);
}
