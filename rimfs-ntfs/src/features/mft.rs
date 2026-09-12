// SPDX-License-Identifier: MIT
//! NTFS Master File Table Feature ($MFT)
//!
//! Handles writing Inode 0 ($MFT), Inode 1 ($MFTMirr), Inode 6 ($Bitmap),
//! Inode 7 ($Boot), placeholders 12..15, free records 16..23, and syncing
//! the primary records to $MFTMirr.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::allocator::{FsAllocator, NtfsAllocator, NtfsHandle};
use crate::attr::NtfsFileAttributes;
use crate::constant::*;
use crate::core::bitmap::{BitmapDriver, SimpleBitmapMeta};
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::NtfsMeta;
use crate::types::security::SECURITY_DESCRIPTOR_SYSTEM;
use crate::types::{NtfsAttribute, NtfsAttributeContent, NtfsAttributeType, NtfsMftRecord};
use crate::mft::build_mft_reference;
use crate::utils::encode_runs_to_dataruns;

pub struct NtfsMftFeature {
    bitmap_handle: Option<NtfsHandle>,
    timestamp: u64,
}

impl NtfsMftFeature {
    pub fn new(timestamp: u64) -> Self {
        Self {
            bitmap_handle: None,
            timestamp,
        }
    }

    pub fn build_record_0_mft(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        bitmap_dataruns: Vec<u8>,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let mft_clusters = meta.initial_mft_clusters();
        let mft_bytes = mft_clusters * meta.bytes_per_cluster as u64;

        let content = NtfsAttributeContent::NonResident {
            allocated_size: mft_bytes,
            data_size: mft_bytes,
            initialized_size: mft_bytes,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: mft_clusters - 1,
        };

        let mut record = NtfsMftRecord::new_file(
            0,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$MFT",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        );

        let bitmap_size = meta.reserved_mft_records / 8;
        let clusters = bitmap_size.div_ceil(meta.bytes_per_cluster as u64);

        record.add_attribute(NtfsAttribute {
            attr_type: NtfsAttributeType::Bitmap,
            content: NtfsAttributeContent::NonResident {
                allocated_size: clusters * meta.bytes_per_cluster as u64,
                data_size: bitmap_size,
                initialized_size: bitmap_size,
                dataruns: bitmap_dataruns,
                lowest_vcn: 0,
                highest_vcn: clusters.saturating_sub(1),
            },
            name: "",
            flags: 0,
        });

        record
    }

    pub fn build_record_1_mftmirr(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let mirr_clusters =
            (4u64 * meta.mft_record_size as u64).div_ceil(meta.bytes_per_cluster as u64);
        let bytes = mirr_clusters * meta.bytes_per_cluster as u64;

        let content = NtfsAttributeContent::NonResident {
            allocated_size: bytes,
            data_size: bytes,
            initialized_size: bytes,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: mirr_clusters.saturating_sub(1),
        };

        NtfsMftRecord::new_file(
            1,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$MFTMirr",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn build_record_6_bitmap(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let clusters = meta
            .bitmap_size_bytes
            .div_ceil(meta.bytes_per_cluster as u64);

        let content = NtfsAttributeContent::NonResident {
            allocated_size: clusters * meta.bytes_per_cluster as u64,
            data_size: meta.bitmap_size_bytes,
            initialized_size: meta.bitmap_size_bytes,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: clusters.saturating_sub(1),
        };

        NtfsMftRecord::new_file(
            6,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$Bitmap",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn build_record_7_boot(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let size = 16 * meta.bytes_per_sector as u64;
        let clusters = size.div_ceil(meta.bytes_per_cluster as u64);

        let content = NtfsAttributeContent::NonResident {
            allocated_size: clusters * meta.bytes_per_cluster as u64,
            data_size: size,
            initialized_size: size,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: clusters.saturating_sub(1),
        };

        NtfsMftRecord::new_file(
            7,
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$Boot",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn build_record_placeholder(
        _meta: &NtfsMeta,
        record_number: u32,
        timestamp: u64,
    ) -> NtfsMftRecord<'static> {
        let mut record = NtfsMftRecord::new(record_number, false, true);
        record.header.link_count = (0).into();
        record.add_attribute(NtfsAttribute::standard_info_basic(
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::security_descriptor(
            SECURITY_DESCRIPTOR_SYSTEM.to_bytes(),
        ));
        record.add_attribute(NtfsAttribute::data_empty());
        record
    }

    fn write_mft_mirror<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &NtfsMeta,
    ) -> FsFeatureResult<()> {
        let mirror_offset = meta.lcn_to_offset(meta.mft_mirr_lcn);
        let mft_offset = meta.lcn_to_offset(meta.mft_lcn);

        let mirror_size = if meta.bytes_per_cluster <= 4096 {
            4 * meta.mft_record_size as usize
        } else {
            meta.bytes_per_cluster as usize
        };

        let mut mirror_data = vec![0u8; mirror_size];
        io.read_at(mft_offset, &mut mirror_data)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        io.write_at(mirror_offset, &mirror_data)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO> for NtfsMftFeature {
    fn name(&self) -> &str {
        "NTFS Master File Table ($MFT)"
    }

    fn prepare(&mut self, _meta: &NtfsMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, io: &mut IO, allocator: &mut NtfsAllocator<'a>) -> FsFeatureResult<()> {
        // Pre-zero MFT area on disk
        let meta = allocator.meta;
        let mft_offset = meta.mft_record_offset(0);
        let mft_area_size = meta.reserved_mft_records * meta.mft_record_size as u64;
        io.zero_at(mft_offset, mft_area_size)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        let records_per_cluster = meta.bytes_per_cluster as u64 * 8;
        let bitmap_clusters = meta.reserved_mft_records.div_ceil(records_per_cluster);
        let handle = allocator
            .allocate_contiguous(io, bitmap_clusters)
            .map_err(crate::core::errors::FsFeatureError::Allocator)?;
        self.bitmap_handle = Some(handle);
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;

        let bitmap_handle = self.bitmap_handle.as_ref().ok_or(
            crate::core::errors::FsFeatureError::InvalidConfiguration("MFT bitmap not allocated"),
        )?;

        let mft_clusters = (meta.reserved_mft_records * meta.mft_record_size as u64)
            .div_ceil(meta.bytes_per_cluster as u64);
        let mft_handle = NtfsHandle::from_range(meta.mft_lcn, mft_clusters);
        let mft_dataruns = encode_runs_to_dataruns(&mft_handle.runs);

        let records_per_cluster = meta.bytes_per_cluster as u64 * 8;
        let bitmap_clusters = meta.reserved_mft_records.div_ceil(records_per_cluster);
        let bitmap_offset = meta.lcn_to_offset(bitmap_handle.start_lcn);
        let bitmap_size_bytes = bitmap_clusters * meta.bytes_per_cluster as u64;

        let bm_meta =
            SimpleBitmapMeta::new(bitmap_offset, bitmap_size_bytes, meta.reserved_mft_records);
        let mut driver = BitmapDriver::new(bm_meta);
        driver
            .format_with(io, 0x00)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        driver
            .set_bits_range(io, 0, MFT_RECORD_FREE_START, true)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        driver
            .set_bit(io, MFT_RECORD_QUOTA, true)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        driver
            .set_bit(io, MFT_RECORD_OBJID, true)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        driver
            .set_bit(io, MFT_RECORD_REPARSE, true)
            .map_err(crate::core::errors::FsFeatureError::IO)?;
        driver
            .flush(io)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        let bitmap_dataruns = encode_runs_to_dataruns(&bitmap_handle.runs);
        let record0 =
            Self::build_record_0_mft(meta, mft_dataruns, bitmap_dataruns, SECURITY_ID_SYSTEM);
        record0
            .write_to_mft(io, meta, MFT_RECORD_MFT)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        let mirr_clusters =
            (4 * meta.mft_record_size as u64).div_ceil(meta.bytes_per_cluster as u64);
        let mirr_handle = NtfsHandle::from_range(meta.mft_mirr_lcn, mirr_clusters);
        let mirr_dataruns = encode_runs_to_dataruns(&mirr_handle.runs);
        let record1 = Self::build_record_1_mftmirr(meta, mirr_dataruns, SECURITY_ID_SYSTEM);
        record1
            .write_to_mft(io, meta, MFT_RECORD_MFTMIRR)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        let total_bitmap_size = meta.total_clusters.div_ceil(8);
        let total_bmp_clusters = total_bitmap_size.div_ceil(meta.bytes_per_cluster as u64);
        let bmp_handle = NtfsHandle::from_range(meta.bitmap_lcn, total_bmp_clusters);
        let bmp_dataruns = encode_runs_to_dataruns(&bmp_handle.runs);
        let record6 = Self::build_record_6_bitmap(meta, bmp_dataruns, SECURITY_ID_SYSTEM);
        record6
            .write_to_mft(io, meta, MFT_RECORD_BITMAP)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        let boot_size = 16 * meta.bytes_per_sector as u64;
        let boot_clusters = boot_size.div_ceil(meta.bytes_per_cluster as u64);
        let boot_handle = NtfsHandle::from_range(0, boot_clusters);
        let boot_dataruns = encode_runs_to_dataruns(&boot_handle.runs);
        let record7 = Self::build_record_7_boot(meta, boot_dataruns, SECURITY_ID_SYSTEM);
        record7
            .write_to_mft(io, meta, MFT_RECORD_BOOT)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // Placeholders 12..15 (In-use records with 0x10, 0x50, 0x80)
        for i in MFT_RECORD_RESERVED_START..MFT_RECORD_FREE_START {
            let record = Self::build_record_placeholder(meta, i as u32, self.timestamp);
            record
                .write_to_mft(io, meta, i)
                .map_err(crate::core::errors::FsFeatureError::IO)?;
        }

        // Records 16..23 are free
        for i in MFT_RECORD_FREE_START..MFT_RECORD_USER_START {
            let record = NtfsMftRecord::new(i as u32, false, false);
            record
                .write_to_mft(io, meta, i)
                .map_err(crate::core::errors::FsFeatureError::IO)?;
        }

        // Remaining reserved: write empty records (skipping quota/objid/reparse)
        for i in MFT_RECORD_USER_START..NTFS_RESERVED_MFT_RECORDS {
            if i == MFT_RECORD_OBJID || i == MFT_RECORD_QUOTA || i == MFT_RECORD_REPARSE {
                continue;
            }
            let record = NtfsMftRecord::new(i as u32, false, false);
            record
                .write_to_mft(io, meta, i)
                .map_err(crate::core::errors::FsFeatureError::IO)?;
        }

        // 8. Initial sync to $MFTMirr
        self.write_mft_mirror(io, meta)?;

        Ok(())
    }
}
