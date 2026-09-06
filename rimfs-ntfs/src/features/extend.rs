// SPDX-License-Identifier: MIT
//! NTFS Extend Directory Feature ($Extend)
//!
//! Handles writing Inode 11 ($Extend directory) and its three canonical children:
//! Inode 24 ($Quota with $O and $Q indexes), Inode 25 ($ObjId with $O index),
//! and Inode 26 ($Reparse with $R index).

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::NtfsAllocator;
use crate::attr::NtfsFileNameNamespace;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::flags::{IndexEntryFlags, NtfsFileAttributes};
use crate::meta::NtfsMeta;
use crate::mft;
use crate::system::quota::{new_objid, new_quota, new_reparse};
use crate::types::{IndexEntryHeader, NtfsAttribute, NtfsIndexEntry, NtfsMftRecord};
use crate::utils::build_mft_reference;
use zerocopy::IntoBytes;

pub struct NtfsExtendFeature {
    timestamp: u64,
}

impl NtfsExtendFeature {
    pub fn new(timestamp: u64) -> Self {
        Self { timestamp }
    }

    fn extend_ref() -> u64 {
        build_mft_reference(MFT_RECORD_EXTEND, 11)
    }

    pub fn build_record(
        meta: &NtfsMeta,
        timestamp: u64,
        security_id: u32,
    ) -> NtfsMftRecord<'static> {
        let attrs =
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::DIRECTORY;
        let entries = Self::build_extend_entries(timestamp);

        let mut buf = Vec::new();
        for e in &entries {
            let len = e.len();
            let old = buf.len();
            buf.resize(old + len, 0);
            let mut io = MemRimIO::new(&mut buf[old..]);
            e.write_to_io(&mut io, 0).ok();
        }
        buf.extend_from_slice(IndexEntryHeader::new(0, 0, true).as_bytes());

        let clusters_per_index = if meta.index_record_size >= meta.bytes_per_cluster {
            (meta.index_record_size / meta.bytes_per_cluster) as i8
        } else {
            -(meta.index_record_size.trailing_zeros() as i8)
        };

        let mut record = NtfsMftRecord::new(MFT_RECORD_EXTEND as u32, true, true);
        record.add_attribute(NtfsAttribute::standard_info_custom(
            attrs,
            security_id,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::file_name_custom(
            build_mft_reference(MFT_RECORD_ROOT, 5),
            "$Extend",
            0,
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::I30_INDEX,
            NtfsFileNameNamespace::Win32AndDos,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::index_root_i30(
            buf,
            clusters_per_index,
            meta.index_record_size,
            false,
        ));

        record
    }

    fn build_extend_entries(timestamp: u64) -> Vec<NtfsIndexEntry> {
        let extend_ref = Self::extend_ref();
        let sys = NtfsFileAttributes::HIDDEN
            | NtfsFileAttributes::SYSTEM
            | NtfsFileAttributes::VIEW_INDEX;

        let items: &[(u64, &str, NtfsFileAttributes)] = &[
            (MFT_RECORD_OBJID, "$ObjId", sys),
            (MFT_RECORD_QUOTA, "$Quota", sys),
            (MFT_RECORD_REPARSE, "$Reparse", sys),
        ];

        let mut entries: Vec<NtfsIndexEntry> = items
            .iter()
            .map(|&(rec, name, attrs)| {
                NtfsIndexEntry::new(
                    build_mft_reference(rec, 1),
                    extend_ref,
                    name.encode_utf16().collect(),
                    attrs,
                    IndexEntryFlags::empty(),
                    None,
                )
                .with_timestamps(timestamp)
                .with_namespace(NtfsFileNameNamespace::Win32AndDos)
                .with_sizes(0, 0)
            })
            .collect();

        // Sort case-insensitive ASCII-ish
        entries.sort_by(|a, b| {
            let map_upcase = |&c: &u16| {
                if c >= b'a' as u16 && c <= b'z' as u16 {
                    c - 0x20
                } else {
                    c
                }
            };
            a.name
                .iter()
                .map(map_upcase)
                .cmp(b.name.iter().map(map_upcase))
        });

        entries
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsExtendFeature
{
    fn name(&self) -> &str {
        "NTFS Extend Directory ($Extend)"
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
        let extend_ref = Self::extend_ref();

        // 1. Write $Extend directory (Record 11)
        let record = Self::build_record(meta, self.timestamp, SECURITY_ID_SYSTEM);
        let raw = record
            .to_raw_buffer(meta)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("MFT serialization failed"))?;
        mft::write_record(io, meta, MFT_RECORD_EXTEND, &raw)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 2. Write $Quota (Record 24)
        let quota = new_quota(meta, self.timestamp, extend_ref);
        let raw_quota = quota
            .to_raw_buffer(meta)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("MFT serialization failed"))?;
        mft::write_record(io, meta, MFT_RECORD_QUOTA, &raw_quota)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 3. Write $ObjId (Record 25)
        let objid = new_objid(meta, self.timestamp, extend_ref);
        let raw_objid = objid
            .to_raw_buffer(meta)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("MFT serialization failed"))?;
        mft::write_record(io, meta, MFT_RECORD_OBJID, &raw_objid)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 4. Write $Reparse (Record 26)
        let reparse = new_reparse(meta, self.timestamp, extend_ref);
        let raw_reparse = reparse
            .to_raw_buffer(meta)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("MFT serialization failed"))?;
        mft::write_record(io, meta, MFT_RECORD_REPARSE, &raw_reparse)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}
