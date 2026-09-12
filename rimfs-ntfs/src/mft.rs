// SPDX-License-Identifier: MIT
//! MFT (Master File Table) low-level IO and allocation
//!
//! Provides stateless helpers for reading and writing MFT records,
//! and for managing MFT record number allocation.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};
use rimio::errors::{RimIOError, RimIOResult};
use zerocopy::FromBytes;

use crate::constant::MFT_RECORD_USNJRNL;
use crate::core::allocator::{FsAllocator, FsAllocatorError, FsAllocatorResult, FsHandle};
use crate::core::bitmap::{BitmapDriver, SimpleBitmapMeta};
use crate::meta::NtfsMeta;
use crate::types::{MftRecordHeader, NtfsAttributeType};
use crate::utils::decode_usa_fixup;
use rimio::{RimIO, RimRead};

/// Handle for an MFT record
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MftHandle(pub u64);

impl FsHandle for MftHandle {}

/// Parse MFT record reference into record number and sequence.
pub fn parse_mft_reference(reference: u64) -> (u64, u16) {
    let record_number = reference & 0x0000FFFFFFFFFFFF;
    let sequence = (reference >> 48) as u16;
    (record_number, sequence)
}

/// Build MFT reference from record number and sequence.
pub fn build_mft_reference(record_number: u64, sequence: u16) -> u64 {
    (record_number & 0x0000FFFFFFFFFFFF) | ((sequence as u64) << 48)
}

/// Returns the canonical sequence number for an MFT record number.
pub fn mft_record_sequence_number(mft_num: u64) -> u16 {
    match mft_num {
        0 | 1 => 1,
        2..=15 => mft_num as u16,
        _ => 1,
    }
}

/// Build an MFT reference using the canonical sequence number.
pub fn system_file_mft_reference(mft_num: u64) -> u64 {
    build_mft_reference(mft_num, mft_record_sequence_number(mft_num))
}

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
    let header = MftRecordHeader::ref_from_prefix(&buf)
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
        .find(NtfsAttributeType::Data)
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

    let header = MftRecordHeader::ref_from_prefix(&buf)
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

    let rec0 = read_record_direct(io, meta, 0)?;
    let runs = extract_mft_runs(&rec0)?;
    if runs.is_empty() {
        return Err(RimIOError::Invalid("Missing MFT runs"));
    }
    read_record_from_runs(io, meta, record_number, &runs)
}

/// Write an MFT record by its record number
///
/// Note: caller is responsible for applying USA fixup to the record buffer
/// if it wasn't already done (usually done by NtfsMftRecord::to_raw_buffer).
#[allow(dead_code)]
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
    driver: BitmapDriver<SimpleBitmapMeta>,
    allocated_records: Vec<u64>,
    next_mft_record: u64,
    meta: &'a NtfsMeta,
}

impl<'a> MftAllocator<'a> {
    pub fn new(meta: &'a NtfsMeta, next_mft_record: u64) -> Self {
        let bitmap_meta = SimpleBitmapMeta::new(0, 0, meta.reserved_mft_records);
        Self {
            driver: BitmapDriver::new(bitmap_meta),
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
            .find(NtfsAttributeType::Bitmap)
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
        let cluster_bytes = meta.bytes_per_cluster as u64;

        let bitmap_meta = SimpleBitmapMeta::new(offset, cluster_bytes, meta.reserved_mft_records);
        let mut driver = BitmapDriver::new(bitmap_meta);

        // Find initial next_mft_record: first free bit >= MFT_RECORD_USNJRNL + 1 (28)
        let next_mft_record = driver
            .find_next_free_ro(io, MFT_RECORD_USNJRNL + 1, 1)
            .map_err(FsAllocatorError::IO)?
            .unwrap_or(meta.reserved_mft_records);

        Ok(Self {
            driver,
            allocated_records: Vec::new(),
            next_mft_record,
            meta,
        })
    }

    /// Flush the modified bitmap back to disk.
    pub fn flush<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> FsAllocatorResult<()> {
        if self.driver.meta.size > 0 {
            self.driver.flush(io).map_err(FsAllocatorError::IO)?;
        }
        Ok(())
    }
}

impl<'a> FsAllocator<MftHandle> for MftAllocator<'a> {
    fn allocate<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<MftHandle> {
        self.allocate_contiguous(io, count)
    }

    fn allocate_contiguous<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> FsAllocatorResult<MftHandle> {
        if self.driver.meta.size == 0 {
            let start = self.next_mft_record;
            self.next_mft_record += count;
            return Ok(MftHandle(start));
        }

        let start = self
            .driver
            .find_next_free(io, self.next_mft_record, count)
            .map_err(FsAllocatorError::IO)?
            .ok_or(FsAllocatorError::OutOfBlocks)?;

        if start + count > self.meta.reserved_mft_records {
            return Err(FsAllocatorError::OutOfBlocks);
        }

        self.driver
            .set_bits_range(io, start, count, true)
            .map_err(FsAllocatorError::IO)?;

        for i in 0..count {
            self.allocated_records.push(start + i);
        }
        self.next_mft_record = start + count;
        Ok(MftHandle(start))
    }

    fn used_units(&self) -> u64 {
        if !self.allocated_records.is_empty() {
            self.allocated_records.len() as u64
        } else {
            self.next_mft_record
        }
    }

    fn remaining_units(&self) -> u64 {
        self.meta
            .reserved_mft_records
            .saturating_sub(self.next_mft_record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mft_reference() {
        let record_num = 12345u64;
        let seq = 7u16;

        let reference = build_mft_reference(record_num, seq);
        let (parsed_num, parsed_seq) = parse_mft_reference(reference);

        assert_eq!(parsed_num, record_num);
        assert_eq!(parsed_seq, seq);
    }

    #[test]
    fn test_system_file_mft_reference() {
        assert_eq!(system_file_mft_reference(0), build_mft_reference(0, 1));
        assert_eq!(system_file_mft_reference(5), build_mft_reference(5, 5));
    }
}
