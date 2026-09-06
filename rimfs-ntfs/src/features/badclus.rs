// SPDX-License-Identifier: MIT
//! NTFS Bad Clusters Feature ($BadClus)
//!
//! Handles writing MFT Record 8 ($BadClus) containing an unnamed resident
//! empty $DATA stream and a named non-resident sparse $Bad stream covering
//! the entire volume (total_clusters) as an unallocated hole.

use rimio::prelude::*;

use crate::allocator::NtfsAllocator;
use crate::attr::NtfsFileNameNamespace;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::flags::NtfsFileAttributes;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::{NtfsAttribute, NtfsMftRecord};
use crate::utils::build_mft_reference;

#[derive(Default)]
pub struct NtfsBadClusFeature;

impl NtfsBadClusFeature {
    pub fn new() -> Self {
        Self
    }

    pub fn build_record(meta: &NtfsMeta, security_id: u32) -> NtfsMftRecord<'static> {
        let mut record = NtfsMftRecord::new(MFT_RECORD_BADCLUS as u32, false, true);
        let attrs = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        record.add_attribute(NtfsAttribute::standard_info(attrs, security_id));
        record.add_attribute(NtfsAttribute::file_name(
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$BadClus",
            0,
            attrs,
            NtfsFileNameNamespace::Win32AndDos,
        ));
        record.add_attribute(NtfsAttribute::data_empty());
        record.add_attribute(NtfsAttribute::sparse_badclus(meta));
        record
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsBadClusFeature
{
    fn name(&self) -> &str {
        "NTFS Bad Clusters ($BadClus)"
    }

    fn prepare(&mut self, _meta: &NtfsMeta) -> FsFeatureResult<()> {
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
        let record = Self::build_record(meta, SECURITY_ID_SYSTEM);
        let raw = record
            .to_raw_buffer(meta)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("MFT serialization failed"))?;
        mft::write_record(io, meta, MFT_RECORD_BADCLUS, &raw)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        Ok(())
    }
}
