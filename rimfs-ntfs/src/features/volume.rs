// SPDX-License-Identifier: MIT
//! NTFS Volume Feature ($Volume)
//!
//! Handles writing MFT Record 3 ($Volume), including the volume name,
//! NTFS version (3.1), and volume state flags.

use rimio::prelude::*;

use crate::allocator::NtfsAllocator;
use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::flags::NtfsFileAttributes;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::{NtfsAttribute, NtfsAttributeContent, NtfsMftRecord};
use crate::utils::build_mft_reference;
use zerocopy::IntoBytes;

#[derive(Default)]
pub struct NtfsVolumeFeature;

impl NtfsVolumeFeature {
    pub fn new() -> Self {
        Self
    }

    pub fn build_record(meta: &NtfsMeta, security_id: u32) -> NtfsMftRecord<'static> {
        let mut record = NtfsMftRecord::new(MFT_RECORD_VOLUME as u32, false, true);
        let attrs = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        record.add_attribute(NtfsAttribute::standard_info(attrs, security_id));
        record.add_attribute(NtfsAttribute::file_name(
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$Volume",
            0,
            attrs,
            NtfsFileNameNamespace::Win32AndDos,
        ));

        record.add_attribute(NtfsAttribute {
            attr_type: AttributeType::VolumeName,
            content: NtfsAttributeContent::Resident(
                meta.volume_label[..meta.volume_label_len as usize]
                    .iter()
                    .flat_map(|&c| c.to_le_bytes())
                    .collect(),
            ),
            name: "",
            flags: 0,
        });

        let info = crate::types::VolumeInformation::default();
        record.add_attribute(NtfsAttribute {
            attr_type: AttributeType::VolumeInformation,
            content: NtfsAttributeContent::Resident(info.as_bytes().to_vec()),
            name: "",
            flags: 0,
        });

        record.add_attribute(NtfsAttribute::data_empty());
        record
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsVolumeFeature
{
    fn name(&self) -> &str {
        "NTFS Volume ($Volume)"
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
        mft::write_record(io, meta, MFT_RECORD_VOLUME, &raw)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        Ok(())
    }
}
