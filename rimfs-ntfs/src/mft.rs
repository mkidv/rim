// SPDX-License-Identifier: MIT
//! MFT (Master File Table) low-level IO and allocation
//!
//! Provides stateless helpers for reading and writing MFT records,
//! and for managing MFT record number allocation.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};
use rimio::errors::{RimIOError, RimIOResult};
use zerocopy::FromBytes;

use crate::core::allocator::{FsAllocator, FsAllocatorError, FsAllocatorResult, FsHandle};
use crate::meta::NtfsMeta;
use crate::types::MftRecordHeader;
use crate::utils::decode_usa_fixup;
use rimio::{RimIO, RimRead};

/// Handle for an MFT record
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MftHandle(pub u64);

impl FsHandle for MftHandle {}

/// Read an MFT record directly using contiguous offset calculation
pub fn read_record_direct<IO: RimRead + ?Sized>(
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

/// Extract MFT data runs from Record 0 ($MFT)
pub fn extract_mft_runs(rec0: &[u8]) -> RimIOResult<Vec<crate::view::runlist::NtfsRun>> {
    let view = crate::view::mft_view::MftRecordView::new(rec0)
        .map_err(|_| RimIOError::Invalid("Failed to parse Record 0"))?;
    let attr = view
        .find(crate::constant::ATTR_DATA)
        .map_err(|_| RimIOError::Invalid("Failed to find $DATA in Record 0"))?
        .ok_or(RimIOError::Invalid("Record 0 has no $DATA"))?;

    let attr_view = attr
        .as_view()
        .map_err(|_| RimIOError::Invalid("Malformed $DATA in Record 0"))?;

    match attr_view {
        crate::view::attr_view::AttrView::NonResident { runlist, .. } => {
            Ok(runlist.iter().collect())
        }
        _ => Ok(Vec::new()),
    }
}

/// Read an MFT record using runlist
pub fn read_record_from_runs<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &NtfsMeta,
    record_number: u64,
    runs: &[crate::view::runlist::NtfsRun],
) -> RimIOResult<Vec<u8>> {
    let rec_size = meta.mft_record_size as usize;
    let mft_byte_offset = record_number * meta.mft_record_size as u64;
    let cluster_size = meta.bytes_per_cluster as u64;

    let mut buf = vec![0u8; rec_size];
    let mut bytes_read = 0usize;
    let mut current_vcn = 0u64;

    for run in runs {
        let run_end_vcn = current_vcn + run.len;
        let run_start_byte = current_vcn * cluster_size;
        let run_end_byte = run_end_vcn * cluster_size;

        let target_start = mft_byte_offset + bytes_read as u64;
        let target_end = mft_byte_offset + rec_size as u64;

        if target_start < run_end_byte && target_end > run_start_byte {
            let chunk_start = target_start.max(run_start_byte);
            let chunk_end = target_end.min(run_end_byte);
            let chunk_len = (chunk_end - chunk_start) as usize;

            let in_run_offset = chunk_start - run_start_byte;
            if let Some(lcn) = run.lcn {
                let disk_offset = meta.lcn_to_offset(lcn) + in_run_offset;
                io.read_at(disk_offset, &mut buf[bytes_read..bytes_read + chunk_len])?;
            } else {
                buf[bytes_read..bytes_read + chunk_len].fill(0);
            }
            bytes_read += chunk_len;
            if bytes_read >= rec_size {
                break;
            }
        }
        current_vcn = run_end_vcn;
    }

    if bytes_read < rec_size {
        return Err(RimIOError::Invalid(
            "MFT record beyond MFT stream allocation",
        ));
    }

    let header = MftRecordHeader::read_from_prefix(&buf)
        .map_err(|_| RimIOError::Invalid("Failed to read MFT header"))?
        .0;

    if !header.is_file_record() {
        return Err(RimIOError::Invalid("Invalid MFT signature"));
    }

    if !decode_usa_fixup(&mut buf, meta.bytes_per_sector as usize) {
        return Err(RimIOError::Invalid("Failed to decode MFT USA fixup"));
    }

    Ok(buf)
}

/// Read an MFT record by its record number.
/// Record 0 is read directly at mft_lcn; subsequent records are read using
/// record 0's $DATA run list if present, falling back to contiguous calculation.
pub fn read_record<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &NtfsMeta,
    record_number: u64,
) -> RimIOResult<Vec<u8>> {
    if record_number == 0 {
        return read_record_direct(io, meta, 0);
    }

    if let Ok(rec0) = read_record_direct(io, meta, 0)
        && let Ok(runs) = extract_mft_runs(&rec0)
        && !runs.is_empty()
    {
        return read_record_from_runs(io, meta, record_number, &runs);
    }

    read_record_direct(io, meta, record_number)
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

/// Allocator for MFT record numbers with bitmap awareness
pub struct MftAllocator<'a> {
    bitmap: Vec<u8>,
    bitmap_offset: u64,
    allocated_records: Vec<u64>,
    next_mft_record: u64,
    meta: &'a NtfsMeta,
}

impl<'a> MftAllocator<'a> {
    pub fn new(meta: &'a NtfsMeta, next_mft_record: u64) -> Self {
        Self {
            bitmap: Vec::new(),
            bitmap_offset: 0,
            allocated_records: Vec::new(),
            next_mft_record,
            meta,
        }
    }

    /// Read $MFT::$BITMAP from disk to initialize state.
    pub fn from_io<IO: RimRead + ?Sized>(
        io: &mut IO,
        meta: &'a NtfsMeta,
    ) -> FsAllocatorResult<Self> {
        let rec0 = read_record(io, meta, 0).map_err(FsAllocatorError::IO)?;
        let view = crate::view::mft_view::MftRecordView::new(&rec0)
            .map_err(|_| FsAllocatorError::Other("Failed to parse Record 0"))?;
        let attr = view
            .find(crate::constant::ATTR_BITMAP)
            .map_err(|_| FsAllocatorError::Other("Failed to find $BITMAP in Record 0"))?
            .ok_or(FsAllocatorError::Other("Record 0 has no $BITMAP"))?;

        let attr_view = attr
            .as_view()
            .map_err(|_| FsAllocatorError::Other("Malformed $BITMAP in Record 0"))?;

        let runlist = match attr_view {
            crate::view::attr_view::AttrView::NonResident { runlist, .. } => runlist,
            _ => {
                return Err(FsAllocatorError::Other(
                    "$BITMAP in Record 0 is unexpectedly resident",
                ));
            }
        };

        let mut first_lcn = None;
        for run in runlist.iter() {
            if let Some(lcn) = run.lcn {
                first_lcn = Some(lcn);
                break;
            }
        }
        let lcn = first_lcn.ok_or(FsAllocatorError::Other("$BITMAP has no valid cluster run"))?;
        let offset = meta.lcn_to_offset(lcn);

        let cluster_bytes = meta.bytes_per_cluster as usize;
        let mut bitmap = vec![0u8; cluster_bytes];
        io.read_at(offset, &mut bitmap)
            .map_err(FsAllocatorError::IO)?;

        // Find initial next_mft_record: first free bit >= MFT_RECORD_USNJRNL + 1 (28)
        let mut next_mft_record = crate::constant::MFT_RECORD_USNJRNL + 1;
        let total_bits = (bitmap.len() * 8) as u64;
        while next_mft_record < total_bits && next_mft_record < meta.reserved_mft_records {
            let byte_idx = (next_mft_record / 8) as usize;
            let bit_idx = (next_mft_record % 8) as u8;
            if (bitmap[byte_idx] & (1 << bit_idx)) == 0 {
                break;
            }
            next_mft_record += 1;
        }

        Ok(Self {
            bitmap,
            bitmap_offset: offset,
            allocated_records: Vec::new(),
            next_mft_record,
            meta,
        })
    }

    /// Flush the modified bitmap back to disk.
    pub fn flush<IO: RimIO + ?Sized>(&self, io: &mut IO) -> FsAllocatorResult<()> {
        if self.bitmap_offset > 0 && !self.bitmap.is_empty() {
            io.write_at(self.bitmap_offset, &self.bitmap)
                .map_err(FsAllocatorError::IO)?;
        }
        Ok(())
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
        if self.bitmap.is_empty() {
            let start = self.next_mft_record;
            self.next_mft_record += count as u64;
            return Ok(MftHandle(start));
        }

        let total_bits = (self.bitmap.len() * 8) as u64;
        let mut curr = self.next_mft_record;

        'outer: while curr + count as u64 <= total_bits
            && curr + count as u64 <= self.meta.reserved_mft_records
        {
            for i in 0..count {
                let rec = curr + i as u64;
                let byte_idx = (rec / 8) as usize;
                let bit_idx = (rec % 8) as u8;
                if (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                    curr = rec + 1;
                    continue 'outer;
                }
            }

            let start = curr;
            for i in 0..count {
                let rec = start + i as u64;
                let byte_idx = (rec / 8) as usize;
                let bit_idx = (rec % 8) as u8;
                self.bitmap[byte_idx] |= 1 << bit_idx;
                self.allocated_records.push(rec);
            }
            self.next_mft_record = start + count as u64;
            return Ok(MftHandle(start));
        }

        Err(FsAllocatorError::OutOfBlocks)
    }

    fn used_units(&self) -> usize {
        if !self.allocated_records.is_empty() {
            self.allocated_records.len()
        } else {
            self.next_mft_record as usize
        }
    }

    fn remaining_units(&self) -> usize {
        (self.meta.reserved_mft_records - self.next_mft_record) as usize
    }
}
