// SPDX-License-Identifier: MIT
//! NTFS $Extend children builders: $Quota, $ObjId, $Reparse.
//!
//! Windows 11 expects these files to be present under $Extend, with exact
//! Collation Rules (0x10 for $Q, 0x11 for $O in $Quota, 0x13 for $ObjId and $Reparse),
//! and without any $DATA attribute.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::attr::NtfsFileNameNamespace;
use crate::constant::*;
use crate::flags::{NtfsFileAttributes, QuotaFlags};
use crate::meta::NtfsMeta;
use crate::types::{
    NtfsAttribute, NtfsCollationRule, NtfsMftRecord, QUOTA_OWNER_ID_ADMINS, QUOTA_OWNER_ID_DEFAULT,
    QuotaOEntryData, QuotaQData, SID_ADMINISTRATORS,
};
use zerocopy::IntoBytes;

/// Build the $O index entries for $Quota (Collation Sid = 0x11).
///
/// Contains an entry mapping Admin SID (S-1-5-32-544) to Owner ID 256 (0x100),
/// followed by the 16-byte LAST_ENTRY terminator.
pub fn build_quota_o_entries() -> Vec<u8> {
    let mut buf = Vec::new();
    // Entry 1 (40 bytes):
    // data_offset: 32 (0x20), data_length: 4 (0x04), reserved: 4 bytes 0
    buf.extend_from_slice(&0x0020u16.to_le_bytes());
    buf.extend_from_slice(&0x0004u16.to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]);
    // entry_length: 40 (0x28), key_length: 16 (0x10), flags: 0
    buf.extend_from_slice(&0x0028u16.to_le_bytes());
    buf.extend_from_slice(&(SID_ADMINISTRATORS.len() as u16).to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]);
    // Key SID: S-1-5-32-544 (16 bytes)
    buf.extend_from_slice(&SID_ADMINISTRATORS);
    // Data: Owner ID = 256 (0x100), unknown = 32
    let o_data = QuotaOEntryData::new(QUOTA_OWNER_ID_ADMINS, 32);
    buf.extend_from_slice(o_data.as_bytes());

    // Entry 2 (End marker, 16 bytes):
    buf.extend_from_slice(crate::types::IndexEntryHeader::end_marker().as_bytes());
    buf
}

/// Build the $Q index entries for $Quota (Collation Ulong = 0x10).
///
/// Contains the mandatory Default Quota Record (Owner ID = 1) and the Administrator
/// Record (Owner ID = 256), followed by the LAST_ENTRY terminator.
pub fn build_quota_q_entries(timestamp: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    let default_q = QuotaQData::new_unlimited(QuotaFlags::DEFAULT_LIMITS.bits(), timestamp);

    // Entry 1: Default Quota Record (72 bytes):
    // data_offset: 20 (0x14), data_length: 48 (0x30), reserved: 4 bytes 0
    buf.extend_from_slice(&0x0014u16.to_le_bytes());
    buf.extend_from_slice(&(core::mem::size_of::<QuotaQData>() as u16).to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]);
    // entry_length: 72 (0x48), key_length: 4 (0x04), flags: 0
    buf.extend_from_slice(&0x0048u16.to_le_bytes());
    buf.extend_from_slice(&0x0004u16.to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]);
    // Key: Owner ID = 1
    buf.extend_from_slice(&QUOTA_OWNER_ID_DEFAULT.to_le_bytes());
    // Data (48 bytes):
    buf.extend_from_slice(default_q.as_bytes());
    buf.extend_from_slice(&[0u8; 4]); // padding to 72-byte boundary

    // Entry 2: Administrator Record (Owner ID = 256, 88 bytes):
    let admin_q = QuotaQData::new_unlimited(QuotaFlags::DEFAULT_LIMITS.bits(), timestamp);
    let admin_data_len = (core::mem::size_of::<QuotaQData>() + SID_ADMINISTRATORS.len()) as u16;
    buf.extend_from_slice(&0x0014u16.to_le_bytes()); // data_offset = 20
    buf.extend_from_slice(&admin_data_len.to_le_bytes()); // data_length = 64
    buf.extend_from_slice(&[0u8; 4]);
    buf.extend_from_slice(&0x0058u16.to_le_bytes()); // entry_length = 88
    buf.extend_from_slice(&0x0004u16.to_le_bytes()); // key_length = 4
    buf.extend_from_slice(&[0u8; 4]);
    // Key: Owner ID = 256
    buf.extend_from_slice(&QUOTA_OWNER_ID_ADMINS.to_le_bytes());
    // Data (64 bytes): QuotaQData (48) + SID_ADMINISTRATORS (16)
    buf.extend_from_slice(admin_q.as_bytes());
    buf.extend_from_slice(&SID_ADMINISTRATORS);
    buf.extend_from_slice(&[0u8; 4]); // padding to 88-byte boundary

    // Entry 3: End marker (16 bytes)
    buf.extend_from_slice(crate::types::IndexEntryHeader::end_marker().as_bytes());
    buf
}

/// Helper to calculate clusters per index record for an index root attribute.
fn clusters_per_index(meta: &NtfsMeta) -> i8 {
    if meta.index_record_size >= meta.bytes_per_cluster {
        (meta.index_record_size / meta.bytes_per_cluster) as i8
    } else {
        -(meta.index_record_size.trailing_zeros() as i8)
    }
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

    // $O index root (Collation Sid = 0x11)
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

    // $Q index root (Collation Ulong = 0x10) with default quota record
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
