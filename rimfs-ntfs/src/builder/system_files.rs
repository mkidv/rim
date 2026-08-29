// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::builder::{NtfsAttribute, NtfsAttributeContent, NtfsMftRecord};
use crate::constant::*;
use crate::meta::NtfsMeta;
use crate::types::NtfsFileAttributes;
use crate::utils::build_mft_reference;
use zerocopy::IntoBytes;

#[inline]
pub fn sys_ref(record: u64) -> u64 {
    let seq = match record {
        0 | 1 => 1,
        2..=11 => record as u16,
        _ => 1,
    };
    build_mft_reference(record, seq)
}

#[inline]
pub fn root_ref() -> u64 {
    sys_ref(MFT_RECORD_ROOT)
}

impl<'a> NtfsMftRecord<'a> {
    /// Create a directory record with $I30 index root.
    #[allow(clippy::too_many_arguments)]
    pub fn new_dir(
        mft_num: u32,
        parent_ref: u64,
        name: &str,
        index_entries: Vec<u8>,
        has_children: bool,
        attrs: NtfsFileAttributes,
        meta: &NtfsMeta,
        security_id: u32,
    ) -> Self {
        let mut record = Self::new(mft_num, true, true);

        record.add_attribute(NtfsAttribute::standard_info(attrs, security_id));
        record.add_attribute(NtfsAttribute::file_name(
            parent_ref,
            name,
            0,
            attrs,
            NtfsFileNameNamespace::Win32AndDos,
        ));

        let clusters_per_index = meta.clusters_per_index_record_raw();

        // Ensure the caller provided a terminator; if buffer is empty, provide a default terminator.
        let mut entries = index_entries;
        if entries.is_empty() {
            entries.extend_from_slice(&NtfsAttribute::index_end_marker());
        }

        record.add_attribute(NtfsAttribute::index_root_i30(
            entries,
            clusters_per_index,
            meta.index_record_size,
            has_children,
        ));

        record
    }

    /// Create a file record with a $DATA attribute.
    pub fn new_file(
        mft_num: u32,
        parent_ref: u64,
        name: &str,
        attrs: NtfsFileAttributes,
        content: NtfsAttributeContent,
        security_id: u32,
    ) -> Self {
        let mut record = Self::new(mft_num, false, true);

        record.add_attribute(NtfsAttribute::standard_info(attrs, security_id));

        let (data_size, allocated_size) = match &content {
            NtfsAttributeContent::Resident(v) => (v.len() as u64, v.len() as u64),
            NtfsAttributeContent::NonResident {
                data_size,
                allocated_size,
                ..
            } => (*data_size, *allocated_size),
            _ => (0, 0),
        };

        record.add_attribute(NtfsAttribute::file_name_with_sizes(
            parent_ref,
            name,
            allocated_size,
            data_size,
            attrs,
            NtfsFileNameNamespace::Win32AndDos,
        ));

        record.add_attribute(NtfsAttribute {
            attr_type: AttributeType::Data,
            content,
            name: "",
            flags: 0,
        });

        record
    }

    pub fn new_mft(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        bitmap_dataruns: Vec<u8>,
        security_id: u32,
    ) -> Self {
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

        let mut record = Self::new_file(
            0,
            root_ref(),
            "$MFT",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        );

        // $BITMAP stream for $MFT.
        let bitmap_size = meta.reserved_mft_records / 8;
        let clusters = bitmap_size.div_ceil(meta.bytes_per_cluster as u64);

        record.add_attribute(NtfsAttribute {
            attr_type: AttributeType::Bitmap,
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

    pub fn new_mftmirr(meta: &NtfsMeta, dataruns: Vec<u8>, security_id: u32) -> Self {
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

        Self::new_file(
            1,
            root_ref(),
            "$MFTMirr",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn new_logfile(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        log_size: u64,
        security_id: u32,
    ) -> Self {
        let clusters = log_size.div_ceil(meta.bytes_per_cluster as u64);

        let content = NtfsAttributeContent::NonResident {
            allocated_size: clusters * meta.bytes_per_cluster as u64,
            data_size: log_size,
            initialized_size: log_size,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: clusters.saturating_sub(1),
        };

        Self::new_file(
            2,
            root_ref(),
            "$LogFile",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn new_volume(meta: &NtfsMeta, security_id: u32) -> Self {
        let mut record = Self::new(3, false, true);
        let attrs = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        record.add_attribute(NtfsAttribute::standard_info(attrs, security_id));
        record.add_attribute(NtfsAttribute::file_name(
            root_ref(),
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

        let info = crate::attr::build_volume_information();
        record.add_attribute(NtfsAttribute {
            attr_type: AttributeType::VolumeInformation,
            content: NtfsAttributeContent::Resident(info.as_bytes().to_vec()),
            name: "",
            flags: 0,
        });

        record.add_attribute(NtfsAttribute::data_empty());
        record
    }

    pub fn new_attrdef(content: Vec<u8>, security_id: u32) -> Self {
        Self::new_file(
            4,
            root_ref(),
            "$AttrDef",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            NtfsAttributeContent::Resident(content),
            security_id,
        )
    }

    pub fn new_bitmap_file(meta: &NtfsMeta, dataruns: Vec<u8>, security_id: u32) -> Self {
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

        Self::new_file(
            6,
            root_ref(),
            "$Bitmap",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn new_boot_file(meta: &NtfsMeta, dataruns: Vec<u8>, security_id: u32) -> Self {
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

        Self::new_file(
            7,
            root_ref(),
            "$Boot",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn new_badclus(security_id: u32) -> Self {
        let mut record = Self::new(8, false, true);
        let attrs = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        record.add_attribute(NtfsAttribute::standard_info(attrs, security_id));
        record.add_attribute(NtfsAttribute::file_name(
            root_ref(),
            "$BadClus",
            0,
            attrs,
            NtfsFileNameNamespace::Win32AndDos,
        ));
        record.add_attribute(NtfsAttribute::data_empty());
        record
    }

    pub fn new_secure(
        meta: &NtfsMeta,
        sds_runs: &rimio::run::RunList,
        sds_data_size: u64,
        sii_entries: Vec<u8>,
        sdh_entries: Vec<u8>,
        timestamp: u64,
        security_id: u32,
    ) -> Self {
        let mut record = Self::new(9, false, true);
        let attrs = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        record.add_attribute(NtfsAttribute::standard_info_custom(
            attrs,
            security_id,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::file_name_custom(
            root_ref(),
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

    pub fn new_upcase(
        meta: &NtfsMeta,
        dataruns: Vec<u8>,
        data_size: u64,
        security_id: u32,
    ) -> Self {
        let clusters = data_size.div_ceil(meta.bytes_per_cluster as u64);
        let content = NtfsAttributeContent::NonResident {
            allocated_size: clusters * meta.bytes_per_cluster as u64,
            data_size,
            initialized_size: data_size,
            dataruns,
            lowest_vcn: 0,
            highest_vcn: clusters.saturating_sub(1),
        };

        Self::new_file(
            10,
            root_ref(),
            "$UpCase",
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM,
            content,
            security_id,
        )
    }

    pub fn new_extend(meta: &NtfsMeta, timestamp: u64, security_id: u32) -> Self {
        let attrs =
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::DIRECTORY;

        // Build entries externally and pass serialized form; or keep your existing builder.
        let entries = Self::build_extend_entries(timestamp);

        let mut buf = Vec::new();
        for e in &entries {
            let len = e.len();
            let old = buf.len();
            buf.resize(old + len, 0);
            let mut io = rimio::prelude::MemRimIO::new(&mut buf[old..]);
            e.write_to_io(&mut io, 0).ok();
        }
        buf.extend_from_slice(zerocopy::IntoBytes::as_bytes(
            &crate::types::IndexEntryHeader::new(0, 0, true),
        ));

        let clusters_per_index = if meta.index_record_size >= meta.bytes_per_cluster {
            (meta.index_record_size / meta.bytes_per_cluster) as i8
        } else {
            -(meta.index_record_size.trailing_zeros() as i8)
        };

        let mut record = Self::new(11, true, true);
        record.add_attribute(NtfsAttribute::standard_info_custom(
            attrs,
            security_id,
            timestamp,
        ));
        record.add_attribute(NtfsAttribute::file_name_custom(
            root_ref(),
            "$Extend",
            0,
            attrs,
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

    fn build_extend_entries(timestamp: u64) -> Vec<crate::types::NtfsIndexEntry> {
        use crate::types::{IndexEntryFlags, NtfsIndexEntry};

        let extend_ref = sys_ref(MFT_RECORD_EXTEND);
        let sys = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;

        let items: &[(u64, &str, NtfsFileAttributes)] = &[
            (MFT_RECORD_OBJID, "$ObjId", sys),
            (MFT_RECORD_QUOTA, "$Quota", sys),
            (MFT_RECORD_REPARSE, "$Reparse", sys),
        ];

        let mut entries: Vec<NtfsIndexEntry> = items
            .iter()
            .map(|&(rec, name, attrs)| {
                NtfsIndexEntry::new(
                    sys_ref(rec),
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

        // Sort case-insensitive ASCII-ish (good enough for $Extend children).
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
