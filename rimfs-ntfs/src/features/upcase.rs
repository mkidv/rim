// SPDX-License-Identifier: MIT
//! NTFS Upcase Table Feature ($UpCase)
//!
//! Handles writing MFT Record 10 ($UpCase) and the 128 KB Unicode collation
//! table to disk, alongside the 32-byte resident named stream $Info.

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
use crate::types::{NtfsAttribute, NtfsAttributeContent, NtfsMftRecord};
use crate::upcase::UpcaseHandle;
use crate::utils::encode_runs_to_dataruns;

#[derive(Default)]
pub struct NtfsUpCaseFeature {
    clusters: u64,
}

impl NtfsUpCaseFeature {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_record(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        data_size: u64,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let clusters = data_size.div_ceil(meta.bytes_per_cluster as u64);
        let content = NtfsAttributeContent::NonResident {
            allocated_size: clusters * meta.bytes_per_cluster as u64,
            data_size,
            initialized_size: data_size,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: clusters.saturating_sub(1),
        };

        let mut record = NtfsMftRecord::new_file(
            MFT_RECORD_UPCASE as u32,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$UpCase",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        );

        // Standard NTFS 3.1 $UpCase:$Info stream (32-byte version info expected by Windows chkdsk)
        let info_data = [
            0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x69, 0x1b, 0x6b, 0x77, 0x7e,
            0xdc, 0xda, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        record.add_attribute(NtfsAttribute::data_resident_named(
            "$Info",
            info_data.to_vec(),
        ));

        record
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsUpCaseFeature
{
    fn name(&self) -> &str {
        "NTFS Upcase Table ($UpCase)"
    }

    fn prepare(&mut self, meta: &NtfsMeta) -> FsFeatureResult<()> {
        self.clusters = (128 * 1024u64).div_ceil(meta.bytes_per_cluster as u64);
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
        let upcase = UpcaseHandle::from_flavor(&meta.upcase_flavor);
        let bytes = upcase.as_bytes();
        let size = bytes.len() as u64;

        let handle = NtfsHandle::from_range(meta.upcase_lcn(), self.clusters);

        let mut bytes_vec = bytes.to_vec();
        let mut stream = MemRimIO::new(&mut bytes_vec);
        crate::core::utils::stream_copy::write_stream_to_run_list(
            io,
            meta,
            &mut stream,
            &handle.runs,
            size,
        )
        .map_err(crate::core::errors::FsFeatureError::IO)?;

        let dataruns = encode_runs_to_dataruns(&handle.runs);
        let record = Self::build_record(meta, dataruns, size, SECURITY_ID_SYSTEM);

        record
            .write_to_mft(io, meta, MFT_RECORD_UPCASE)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}
