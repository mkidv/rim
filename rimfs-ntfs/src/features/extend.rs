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
use crate::attr::NtfsFileAttributes;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::flags::IndexEntryFlags;
use crate::meta::NtfsMeta;
use crate::mft::build_mft_reference;
use crate::types::{
    IndexDataEntryHeader, IndexEntryHeader, NtfsAttribute, NtfsCollationRule,
    NtfsFileNameNamespace, NtfsIndexEntry, NtfsMftRecord, QUOTA_OWNER_ID_ADMINS,
    QUOTA_OWNER_ID_DEFAULT, QuotaFlags, QuotaOEntryData, QuotaQData,
};
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

        let clusters_per_index = clusters_per_index(meta);

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
        record
            .write_to_mft(io, meta, MFT_RECORD_EXTEND)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 2. Write $Quota (Record 24)
        let quota = new_quota(meta, self.timestamp, extend_ref);
        quota
            .write_to_mft(io, meta, MFT_RECORD_QUOTA)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 3. Write $ObjId (Record 25)
        let objid = new_objid(meta, self.timestamp, extend_ref);
        objid
            .write_to_mft(io, meta, MFT_RECORD_OBJID)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        // 4. Write $Reparse (Record 26)
        let reparse = new_reparse(meta, self.timestamp, extend_ref);
        reparse
            .write_to_mft(io, meta, MFT_RECORD_REPARSE)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}

/// Helper to calculate clusters per index record for an index root attribute.
fn clusters_per_index(meta: &NtfsMeta) -> i8 {
    if meta.index_record_size >= meta.bytes_per_cluster {
        (meta.index_record_size / meta.bytes_per_cluster) as i8
    } else {
        -(meta.index_record_size.trailing_zeros() as i8)
    }
}

/// Build the $O index entries for $Quota (Collation Sid = 0x11).
pub fn build_quota_o_entries() -> Vec<u8> {
    let mut buf = Vec::new();
    let header = IndexDataEntryHeader {
        data_offset: 32u16.into(),
        data_length: 4u16.into(),
        entry_length: 40u16.into(),
        key_length: (SID_ADMINISTRATORS.len() as u16).into(),
        ..Default::default()
    };
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(SID_ADMINISTRATORS.as_bytes());
    let o_data = QuotaOEntryData::new(QUOTA_OWNER_ID_ADMINS, 32);
    buf.extend_from_slice(o_data.as_bytes());

    buf.extend_from_slice(IndexEntryHeader::end_marker().as_bytes());
    buf
}

/// Build the $Q index entries for $Quota (Collation Ulong = 0x10).
pub fn build_quota_q_entries(timestamp: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    let default_q = QuotaQData::new_unlimited(QuotaFlags::DEFAULT_LIMITS.bits(), timestamp);

    let header = IndexDataEntryHeader {
        data_offset: 20u16.into(),
        data_length: (core::mem::size_of::<QuotaQData>() as u16).into(),
        entry_length: 72u16.into(),
        key_length: 4u16.into(),
        ..Default::default()
    };
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(&QUOTA_OWNER_ID_DEFAULT.to_le_bytes());
    buf.extend_from_slice(default_q.as_bytes());
    buf.extend_from_slice(&[0u8; 4]);

    let admin_q = QuotaQData::new_unlimited(QuotaFlags::DEFAULT_LIMITS.bits(), timestamp);
    let admin_data_len = (core::mem::size_of::<QuotaQData>() + SID_ADMINISTRATORS.len()) as u16;
    let header = IndexDataEntryHeader {
        data_offset: 20u16.into(),
        data_length: admin_data_len.into(),
        entry_length: 88u16.into(),
        key_length: 4u16.into(),
        ..Default::default()
    };
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(&QUOTA_OWNER_ID_ADMINS.to_le_bytes());
    buf.extend_from_slice(admin_q.as_bytes());
    buf.extend_from_slice(SID_ADMINISTRATORS.as_bytes());
    buf.extend_from_slice(&[0u8; 4]);

    buf.extend_from_slice(IndexEntryHeader::end_marker().as_bytes());
    buf
}

/// Create $Quota MFT record (Record 24).
pub fn new_quota(meta: &NtfsMeta, timestamp: u64, extend_ref: u64) -> NtfsMftRecord<'static> {
    let mut record = NtfsMftRecord::new(MFT_RECORD_QUOTA as u32, false, true);
    record.header.flags |= crate::flags::MftRecordFlags::IN_EXTEND.bits()
        | crate::flags::MftRecordFlags::IS_VIEW_INDEX.bits();

    let child_attrs =
        NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::VIEW_INDEX;

    record.add_attribute(NtfsAttribute::standard_info_custom(
        child_attrs,
        SECURITY_ID_SYSTEM,
        timestamp,
    ));
    record.add_attribute(NtfsAttribute::file_name_custom(
        extend_ref,
        "$Quota",
        0,
        child_attrs,
        NtfsFileNameNamespace::Win32AndDos,
        timestamp,
    ));

    let cpi = clusters_per_index(meta);

    let o_entries = build_quota_o_entries();
    record.add_attribute(NtfsAttribute::index_root_named(
        "$O",
        0,
        NtfsCollationRule::Sid.as_u32(),
        o_entries,
        cpi,
        meta.index_record_size,
        false,
    ));

    let q_entries = build_quota_q_entries(timestamp);
    record.add_attribute(NtfsAttribute::index_root_named(
        "$Q",
        0,
        NtfsCollationRule::Ulong.as_u32(),
        q_entries,
        cpi,
        meta.index_record_size,
        false,
    ));

    record
}

/// Create $ObjId MFT record (Record 25).
pub fn new_objid(meta: &NtfsMeta, timestamp: u64, extend_ref: u64) -> NtfsMftRecord<'static> {
    new_simple_extend_child(
        meta,
        MFT_RECORD_OBJID,
        "$ObjId",
        "$O",
        NtfsCollationRule::Ulongs.as_u32(),
        timestamp,
        extend_ref,
    )
}

/// Create $Reparse MFT record (Record 26).
pub fn new_reparse(meta: &NtfsMeta, timestamp: u64, extend_ref: u64) -> NtfsMftRecord<'static> {
    new_simple_extend_child(
        meta,
        MFT_RECORD_REPARSE,
        "$Reparse",
        "$R",
        NtfsCollationRule::Ulongs.as_u32(),
        timestamp,
        extend_ref,
    )
}

fn new_simple_extend_child(
    meta: &NtfsMeta,
    record_number: u64,
    name: &'static str,
    index_name: &'static str,
    collation_rule: u32,
    timestamp: u64,
    extend_ref: u64,
) -> NtfsMftRecord<'static> {
    let mut record = NtfsMftRecord::new(record_number as u32, false, true);
    record.header.flags |= crate::flags::MftRecordFlags::IN_EXTEND.bits()
        | crate::flags::MftRecordFlags::IS_VIEW_INDEX.bits();

    let child_attrs =
        NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::VIEW_INDEX;

    record.add_attribute(NtfsAttribute::standard_info_custom(
        child_attrs,
        SECURITY_ID_SYSTEM,
        timestamp,
    ));
    record.add_attribute(NtfsAttribute::file_name_custom(
        extend_ref,
        name,
        0,
        child_attrs,
        NtfsFileNameNamespace::Win32AndDos,
        timestamp,
    ));

    let cpi = clusters_per_index(meta);
    let end_marker = NtfsAttribute::index_end_marker();
    record.add_attribute(NtfsAttribute::index_root_named(
        index_name,
        0,
        collation_rule,
        end_marker.to_vec(),
        cpi,
        meta.index_record_size,
        false,
    ));

    record
}
