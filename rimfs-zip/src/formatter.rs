// SPDX-License-Identifier: MIT

//! Empty ZIP archive initialization with EOCD record.

use crate::meta::ZipMeta;
use crate::types::{END_OF_CENTRAL_DIR_SIG, ZipEocd};
use rimfs_core::errors::FsFormatterResult;
use rimfs_core::formatter::FsFormatter;
use rimio::{RimIO, RimWriteStructExt};

/// Initializes and writes an empty ZIP archive structure.
pub struct ZipFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a ZipMeta,
}

impl<'a, IO: RimIO + ?Sized> ZipFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> Self {
        Self { io, _meta: meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for ZipFormatter<'a, IO> {
    fn format(&mut self, _full_format: bool) -> FsFormatterResult {
        let eocd = ZipEocd {
            signature: END_OF_CENTRAL_DIR_SIG.into(),
            ..Default::default()
        };
        // Fixed-size images cannot be truncated through RimIO. Remove stale EOCDs
        // throughout the view before publishing the new archive, even in quick mode.
        let size = self.io.total_size()?;
        let end = core::mem::size_of::<ZipEocd>() as u64;
        if size < end {
            return Err(rimio::RimIOError::OutOfBounds.into());
        }
        clean_trailing_storage(self.io, end, size)?;
        self.io.write_struct(0, &eocd)?;
        self.io.flush()?;
        Ok(())
    }
}

/// Neutralizes trailing storage in bounded chunks, skipping blocks that are already all zeros.
pub(crate) fn clean_trailing_storage<IO: RimIO + ?Sized>(
    io: &mut IO,
    from: u64,
    to: u64,
) -> rimio::RimIOResult {
    if from >= to {
        return Ok(());
    }

    const CHUNK_SIZE: usize = 64 * 1024;
    let mut buf = [0u8; CHUNK_SIZE];
    let mut current = from;

    while current < to {
        let len = ((to - current) as usize).min(CHUNK_SIZE);
        io.read_at(current, &mut buf[..len])?;
        if !is_all_zeros(&buf[..len]) {
            io.zero_at(current, len as u64)?;
        }
        current += len as u64;
    }

    Ok(())
}

#[inline]
fn is_all_zeros(buf: &[u8]) -> bool {
    buf.iter().all(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimio::MemRimIO;

    #[test]
    fn clean_trailing_storage_skips_zeros_and_cleans_dirty() {
        let mut data = vec![0u8; 1000];
        data[200..300].fill(0xAA);
        data[800..900].fill(0xBB);
        let mut io = MemRimIO::new(&mut data);

        clean_trailing_storage(&mut io, 100, 1000).unwrap();
        assert_eq!(&data[..100], &[0u8; 100]);
        assert_eq!(&data[100..], &[0u8; 900]);
    }
}
