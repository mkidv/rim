// SPDX-License-Identifier: MIT
//! NTFS Log File Feature ($LogFile)
//!
//! Handles writing MFT Record 2 ($LogFile) and initializing the 0xFF circular
//! transaction logging area on disk (2 MB default).

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::{NtfsAllocator, NtfsHandle};
use crate::attr::NtfsFileAttributes;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::NtfsMeta;
use crate::mft::build_mft_reference;
use crate::types::{NtfsAttributeContent, NtfsMftRecord};
use crate::utils::encode_runs_to_dataruns;

#[derive(Default)]
pub struct NtfsLogFileFeature {
    log_size: u64,
    clusters: u64,
}

impl NtfsLogFileFeature {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_record(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        log_size: u64,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let clusters = log_size.div_ceil(meta.bytes_per_cluster as u64);
        let content = NtfsAttributeContent::NonResident {
            allocated_size: clusters * meta.bytes_per_cluster as u64,
            data_size: log_size,
            initialized_size: log_size,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: clusters.saturating_sub(1),
        };

        NtfsMftRecord::new_file(
            MFT_RECORD_LOGFILE as u32,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$LogFile",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsLogFileFeature
{
    fn name(&self) -> &str {
        "NTFS Log File ($LogFile)"
    }

    fn prepare(&mut self, meta: &NtfsMeta) -> FsFeatureResult<()> {
        self.log_size = (2 * 1024 * 1024u64).min(meta.volume_size_bytes / 10);
        self.clusters = self.log_size.div_ceil(meta.bytes_per_cluster as u64);
        Ok(())
    }

    fn allocate(
        &mut self,
        _io: &mut IO,
        _allocator: &mut NtfsAllocator<'a>,
    ) -> FsFeatureResult<()> {
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let handle = NtfsHandle::from_range(meta.logfile_lcn, self.clusters);

        // 0xFF-init content (standard empty NTFS journal) in 64KB chunks
        let chunk = [0xFFu8; 65536];
        let mut offset = meta.lcn_to_offset(handle.start_lcn);
        let mut remaining = self.clusters * meta.bytes_per_cluster as u64;
        while remaining > 0 {
            let to_write = remaining.min(chunk.len() as u64) as usize;
            io.write_at(offset, &chunk[..to_write])
                .map_err(crate::core::errors::FsFeatureError::IO)?;
            offset += to_write as u64;
            remaining -= to_write as u64;
        }

        let dataruns = encode_runs_to_dataruns(&handle.runs);
        let record = Self::build_record(meta, dataruns, self.log_size, SECURITY_ID_SYSTEM);
        record
            .write_to_mft(io, meta, MFT_RECORD_LOGFILE)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}
