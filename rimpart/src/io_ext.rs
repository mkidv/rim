// SPDX-License-Identifier: MIT

//! BlockIO helpers "LBA-aware" pour éviter les `* sector_size` partout,
//! avec overflow-check et versions `read/write_struct` sur LBA.

use rimio::errors::BlockIOError;
use rimio::prelude::*;

/// Offset = LBA * sector_size (avec overflow-check)
#[inline]
fn lba_offset(lba: u64, sector_size: u64) -> BlockIOResult<u64> {
    lba.checked_mul(sector_size)
        .ok_or(BlockIOError::Other("lba_offset overflow"))
}

/// Lecture/écriture alignée LBA (buffer)
pub trait BlockIOLbaExt: BlockIO {
    /// Lit `buf.len()` octets à partir d’un LBA (offset = lba * sector_size).
    #[inline]
    fn read_at_lba(&mut self, lba: u64, sector_size: u64, buf: &mut [u8]) -> BlockIOResult {
        let off = lba_offset(lba, sector_size)?;
        self.read_at(off, buf)
    }

    /// Écrit `buf.len()` octets à partir d’un LBA (offset = lba * sector_size).
    #[inline]
    fn write_at_lba(&mut self, lba: u64, sector_size: u64, data: &[u8]) -> BlockIOResult {
        let off = lba_offset(lba, sector_size)?;
        self.write_at(off, data)
    }

    /// Lit une struct `T` à partir d’un LBA (taille = size_of::<T>()).
    #[inline]
    fn read_struct_lba<T>(&mut self, lba: u64, sector_size: u64) -> BlockIOResult<T>
    where
        T: zerocopy::FromBytes + zerocopy::KnownLayout + zerocopy::Immutable,
    {
        let off = lba_offset(lba, sector_size)?;
        self.read_struct::<T>(off)
    }

    /// Écrit une struct `T` à partir d’un LBA.
    #[inline]
    fn write_struct_lba<T>(&mut self, lba: u64, sector_size: u64, val: &T) -> BlockIOResult
    where
        T: zerocopy::IntoBytes + zerocopy::KnownLayout + zerocopy::Immutable,
    {
        let off = lba_offset(lba, sector_size)?;
        self.write_struct::<T>(off, val)
    }
}

impl<T: BlockIO + ?Sized> BlockIOLbaExt for T {}
