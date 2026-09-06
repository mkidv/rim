// SPDX-License-Identifier: MIT
//! NTFS Attribute Definitions Feature ($AttrDef)
//!
//! Handles writing MFT Record 4 ($AttrDef) containing the 2560 bytes
//! of canonical attribute definitions and null terminator.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::{FsAllocator, NtfsAllocator, NtfsHandle};
use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::flags::NtfsFileAttributes;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::attrdef::build_standard_attr_defs;
use crate::types::{NtfsAttribute, NtfsAttributeContent, NtfsMftRecord};
use crate::utils::build_mft_reference;

#[derive(Default)]
pub struct NtfsAttrDefFeature {
    content: Vec<u8>,
    handle: Option<NtfsHandle>,
}

impl NtfsAttrDefFeature {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_resident_record(content: Vec<u8>, security_id: u32) -> NtfsMftRecord<'static> {
        NtfsMftRecord::new_file(
            MFT_RECORD_ATTRDEF as u32,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$AttrDef",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            NtfsAttributeContent::Resident(content),
            security_id,
        )
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsAttrDefFeature
{
    fn name(&self) -> &str {
        "NTFS Attribute Definitions ($AttrDef)"
    }

    fn prepare(&mut self, _meta: &NtfsMeta) -> FsFeatureResult<()> {
        self.content = build_standard_attr_defs();
        Ok(())
    }

    fn allocate(&mut self, io: &mut IO, allocator: &mut NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let size = self.content.len() as u64;
        if size >= 600 {
            let clusters = size.div_ceil(allocator.meta.bytes_per_cluster as u64);
            let handle = allocator
                .allocate_contiguous(io, clusters as usize)
                .map_err(crate::core::errors::FsFeatureError::Allocator)?;
            self.handle = Some(handle);
        }
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let size = self.content.len() as u64;

        if let Some(ref handle) = self.handle {
            let mut content_copy = self.content.clone();
            let mut stream = MemRimIO::new(&mut content_copy);
            crate::core::utils::stream_copy::write_stream_to_run_list(
                io,
                meta,
                &mut stream,
                &handle.runs,
                size,
            )
            .map_err(crate::core::errors::FsFeatureError::IO)?;

            let clusters = size.div_ceil(meta.bytes_per_cluster as u64);
            let mut record = NtfsMftRecord::new(MFT_RECORD_ATTRDEF as u32, false, true);
            record.add_attribute(NtfsAttribute::standard_info(
                NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
                SECURITY_ID_SYSTEM,
            ));
            record.add_attribute(NtfsAttribute::file_name_with_sizes(
                build_mft_reference(MFT_RECORD_ROOT, 5),
                "$AttrDef",
                clusters * meta.bytes_per_cluster as u64,
                size,
                NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
                NtfsFileNameNamespace::Win32AndDos,
            ));
            record.add_attribute(NtfsAttribute::non_resident(
                AttributeType::Data,
                "",
                meta,
                &handle.runs,
                size,
            ));

            let raw = record.to_raw_buffer(meta).map_err(|_| {
                crate::core::errors::FsFeatureError::Other("MFT serialization failed")
            })?;
            mft::write_record(io, meta, MFT_RECORD_ATTRDEF, &raw)
                .map_err(crate::core::errors::FsFeatureError::IO)?;
        } else {
            let record = Self::build_resident_record(self.content.clone(), SECURITY_ID_SYSTEM);
            let raw = record.to_raw_buffer(meta).map_err(|_| {
                crate::core::errors::FsFeatureError::Other("MFT serialization failed")
            })?;
            mft::write_record(io, meta, MFT_RECORD_ATTRDEF, &raw)
                .map_err(crate::core::errors::FsFeatureError::IO)?;
        }

        Ok(())
    }
}
