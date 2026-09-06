// SPDX-License-Identifier: MIT
//! NTFS Security Database Feature ($Secure)
//!
//! Handles writing MFT Record 9 ($Secure), containing the $SDS descriptor stream
//! (with SYSTEM and EVERYONE descriptors) and the $SDH and $SII index trees.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::allocator::{FsAllocator, NtfsAllocator, NtfsHandle};
use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::flags::{MftRecordFlags, NtfsFileAttributes};
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::{NtfsAttribute, NtfsMftRecord};
use crate::utils::build_mft_reference;

#[derive(Default)]
pub struct NtfsSecureFeature {
    handle: Option<NtfsHandle>,
    timestamp: u64,
}

impl NtfsSecureFeature {
    pub fn new(timestamp: u64) -> Self {
        Self {
            handle: None,
            timestamp,
        }
    }

    pub fn build_record(
        meta: &NtfsMeta,
        sds_runs: &rimio::run::RunList,
        sds_data_size: u64,
        sii_entries: Vec<u8>,
        sdh_entries: Vec<u8>,
        timestamp: u64,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let mut record = NtfsMftRecord::new(MFT_RECORD_SECURE as u32, false, true);
        record.header.flags |= MftRecordFlags::IS_VIEW_INDEX.bits();
        let attrs = NtfsFileAttributes::HIDDEN
            | NtfsFileAttributes::SYSTEM
            | NtfsFileAttributes::VIEW_INDEX;

        record.add_attribute(NtfsAttribute::standard_info_custom(
            attrs,
            security_id,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::file_name_custom(
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$Secure",
            0,
            attrs,
            NtfsFileNameNamespace::Win32AndDos,
            timestamp,
        ));

        record.add_attribute(NtfsAttribute::non_resident(
            AttributeType::Data,
            "$SDS",
            meta,
            sds_runs,
            sds_data_size,
        ));

        let clusters_per_index = if meta.index_record_size >= meta.bytes_per_cluster {
            (meta.index_record_size / meta.bytes_per_cluster) as i8
        } else {
            -(meta.index_record_size.trailing_zeros() as i8)
        };

        record.add_attribute(NtfsAttribute::index_root_named(
            "$SDH",
            0,
            0x12,
            sdh_entries,
            clusters_per_index,
            meta.index_record_size,
            false,
        ));

        record.add_attribute(NtfsAttribute::index_root_named(
            "$SII",
            0,
            0x10,
            sii_entries,
            clusters_per_index,
            meta.index_record_size,
            false,
        ));

        record
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsSecureFeature
{
    fn name(&self) -> &str {
        "NTFS Security Database ($Secure)"
    }

    fn prepare(&mut self, _meta: &NtfsMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, io: &mut IO, allocator: &mut NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let content = crate::system::secure::build_secure_content();
        let sds_len = content.sds.len() as u64;
        let clusters = sds_len.div_ceil(allocator.meta.bytes_per_cluster as u64);
        let handle = allocator
            .allocate_contiguous(io, clusters as usize)
            .map_err(crate::core::errors::FsFeatureError::Allocator)?;
        self.handle = Some(handle);
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let content = crate::system::secure::build_secure_content();
        let sds_len = content.sds.len() as u64;

        let handle = self.handle.as_ref().ok_or(
            crate::core::errors::FsFeatureError::InvalidConfiguration(
                "Security stream not allocated",
            ),
        )?;

        // Zero-init
        let zero = vec![0u8; meta.bytes_per_cluster as usize];
        let sds_offset = meta.lcn_to_offset(handle.start_lcn);
        let clusters = sds_len.div_ceil(meta.bytes_per_cluster as u64);
        for i in 0..clusters {
            io.write_at(sds_offset + (i * meta.bytes_per_cluster as u64), &zero)
                .map_err(crate::core::errors::FsFeatureError::IO)?;
        }

        // Write $SDS content
        let mut binding = content.sds.clone();
        let mut stream = MemRimIO::new(&mut binding);
        crate::core::utils::stream_copy::write_stream_to_run_list(
            io,
            meta,
            &mut stream,
            &handle.runs,
            sds_len,
        )
        .map_err(crate::core::errors::FsFeatureError::IO)?;

        let record = Self::build_record(
            meta,
            &handle.runs,
            sds_len,
            content.sii_entries,
            content.sdh_entries,
            self.timestamp,
            SECURITY_ID_SYSTEM,
        );

        let raw = record
            .to_raw_buffer(meta)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("MFT serialization failed"))?;
        mft::write_record(io, meta, MFT_RECORD_SECURE, &raw)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}
