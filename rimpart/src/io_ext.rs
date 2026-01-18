// SPDX-License-Identifier: MIT

//! "LBA-aware" RimIO helpers to avoid `* sector_size` everywhere,
//! with overflow-check and `read/write_struct` versions on LBA.

use rimio::errors::RimIOError;
use rimio::prelude::*;

/// Offset = LBA * sector_size (with overflow-check)
#[inline]
fn lba_offset(lba: u64, sector_size: u64) -> RimIOResult<u64> {
    lba.checked_mul(sector_size)
        .ok_or(RimIOError::Other("lba_offset overflow"))
}

/// LBA-aligned read/write (buffer)
pub trait RimIOLbaExt: RimIO {
    /// Reads `buf.len()` bytes starting from an LBA (offset = lba * sector_size).
    #[inline]
    fn read_at_lba(&mut self, lba: u64, sector_size: u64, buf: &mut [u8]) -> RimIOResult {
        let off = lba_offset(lba, sector_size)?;
        self.read_at(off, buf)
    }

    /// Writes `buf.len()` bytes starting from an LBA (offset = lba * sector_size).
    #[inline]
    fn write_at_lba(&mut self, lba: u64, sector_size: u64, data: &[u8]) -> RimIOResult {
        let off = lba_offset(lba, sector_size)?;
        self.write_at(off, data)
    }

    /// Reads a struct `T` starting from an LBA (size = size_of::<T>()).
    #[inline]
    fn read_struct_lba<T>(&mut self, lba: u64, sector_size: u64) -> RimIOResult<T>
    where
        T: zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable,
    {
        let off = lba_offset(lba, sector_size)?;
        self.read_struct::<T>(off)
    }

    /// Writes a struct `T` starting from an LBA.
    #[inline]
    fn write_struct_lba<T>(&mut self, lba: u64, sector_size: u64, val: &T) -> RimIOResult
    where
        T: zerocopy::IntoBytes + zerocopy::KnownLayout + zerocopy::Immutable,
    {
        let off = lba_offset(lba, sector_size)?;
        self.write_struct::<T>(off, val)
    }
}

impl<T: RimIO + ?Sized> RimIOLbaExt for T {}
