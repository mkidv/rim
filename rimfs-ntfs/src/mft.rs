// SPDX-License-Identifier: MIT
//! MFT (Master File Table) low-level IO and allocation
//!
//! Provides stateless helpers for reading and writing MFT records,
//! and for managing MFT record number allocation.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;
use rimio::errors::{RimIOError, RimIOResult};
use zerocopy::FromBytes;

use crate::core::allocator::{FsAllocator, FsAllocatorResult, FsHandle};
use crate::meta::NtfsMeta;
use crate::types::MftRecordHeader;
use crate::utils::decode_usa_fixup;
use rimio::RimIO;

/// Handle for an MFT record
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MftHandle(pub u64);

impl FsHandle for MftHandle {}

/// Read an MFT record by its record number
pub fn read_record<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &NtfsMeta,
    record_number: u64,
) -> RimIOResult<Vec<u8>> {
    let offset = meta.mft_record_offset(record_number);
    let mut buf = vec![0u8; meta.mft_record_size as usize];
    io.read_at(offset, &mut buf)?;

    // Simple validation
    let header = MftRecordHeader::read_from_prefix(&buf)
        .map_err(|_| RimIOError::Invalid("Failed to read MFT header"))?
        .0;

    if !header.is_file_record() {
        return Err(RimIOError::Invalid("Invalid MFT signature"));
    }

    // Apply USA fixups (MFT records always have them)
    if !decode_usa_fixup(&mut buf, meta.bytes_per_sector as usize) {
        return Err(RimIOError::Invalid("Failed to decode MFT USA fixup"));
    }

    Ok(buf)
}

/// Write an MFT record by its record number
///
/// Note: caller is responsible for applying USA fixup to the record buffer
/// if it wasn't already done (usually done by NtfsMftRecord::to_raw_buffer).
pub fn write_record<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &NtfsMeta,
    record_number: u64,
    data: &[u8],
) -> RimIOResult {
    let offset = meta.mft_record_offset(record_number);
    io.write_at(offset, data)?;
    Ok(())
}

/// Simple allocator for MFT record numbers
pub struct MftAllocator<'a> {
    next_mft_record: u64,
    meta: &'a NtfsMeta,
}

impl<'a> MftAllocator<'a> {
    pub fn new(meta: &'a NtfsMeta, next_mft_record: u64) -> Self {
        Self {
            next_mft_record,
            meta,
        }
    }
}

impl<'a> FsAllocator<MftHandle> for MftAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<MftHandle> {
        self.allocate_contiguous(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        _io: &mut IO,
        count: usize,
    ) -> FsAllocatorResult<MftHandle> {
        let start = self.next_mft_record;
        self.next_mft_record += count as u64;
        Ok(MftHandle(start))
    }

    fn used_units(&self) -> usize {
        self.next_mft_record as usize
    }

    fn remaining_units(&self) -> usize {
        (self.meta.reserved_mft_records - self.next_mft_record) as usize
    }
}
