// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use crate::builder::{NtfsAttribute, NtfsAttributeContent};
use crate::meta::NtfsMeta;
use crate::types::{
    AttributeType, FileNameAttribute, IndexNodeHeader, IndexRootHeader, NtfsFileAttributes,
    NtfsFileNameNamespace, StandardInformation, VolumeInformation,
};
use crate::utils::{current_ntfs_time, encode_data_run, encode_data_run_end};
use zerocopy::IntoBytes;

impl<'a> NtfsAttribute<'a> {
    #[inline]
    fn now() -> u64 {
        current_ntfs_time()
    }

    // ---------- $STANDARD_INFORMATION ----------

    pub fn standard_info(attrs: NtfsFileAttributes, security_id: u32) -> Self {
        Self::standard_info_custom(attrs, security_id, Self::now())
    }

    pub fn standard_info_custom(
        attrs: NtfsFileAttributes,
        security_id: u32,
        timestamp: u64,
    ) -> Self {
        let info = StandardInformation {
            creation_time: timestamp,
            modification_time: timestamp,
            mft_modification_time: timestamp,
            access_time: timestamp,
            file_attributes: attrs.bits(),
            maximum_versions: 0,
            version_number: 0,
            class_id: 0,
            owner_id: 0,
            security_id,
            quota_charged: 0,
            usn: 0,
        };

        Self {
            attr_type: AttributeType::StandardInformation,
            content: NtfsAttributeContent::StandardInformation(info),
            name: "",
            flags: 0,
        }
    }

    // ---------- $FILE_NAME ----------

    pub fn file_name(
        parent_ref: u64,
        name: &str,
        data_size: u64,
        attrs: NtfsFileAttributes,
        namespace: NtfsFileNameNamespace,
    ) -> Self {
        Self::file_name_custom(parent_ref, name, data_size, attrs, namespace, Self::now())
    }

    pub fn file_name_custom(
        parent_ref: u64,
        name: &str,
        data_size: u64,
        attrs: NtfsFileAttributes,
        namespace: NtfsFileNameNamespace,
        timestamp: u64,
    ) -> Self {
        let name_u16: Vec<u16> = name.encode_utf16().collect();
        let name_len = name_u16.len() as u8;

        let mut fn_attr = FileNameAttribute::new(
            parent_ref,
            data_size, // file data size (not attribute size)
            attrs,
            name_len,
            namespace.bits(),
        );

        fn_attr.creation_time = timestamp;
        fn_attr.modification_time = timestamp;
        fn_attr.mft_modification_time = timestamp;
        fn_attr.access_time = timestamp;

        Self {
            attr_type: AttributeType::FileName,
            content: NtfsAttributeContent::FileName(fn_attr, name_u16),
            name: "",
            flags: 0,
        }
    }

    // ---------- $DATA helpers ----------

    pub fn data_empty() -> Self {
        Self {
            attr_type: AttributeType::Data,
            content: NtfsAttributeContent::Resident(vec![]),
            name: "",
            flags: 0,
        }
    }

    pub fn data_resident(data: Vec<u8>) -> Self {
        Self {
            attr_type: AttributeType::Data,
            content: NtfsAttributeContent::Resident(data),
            name: "",
            flags: 0,
        }
    }

    pub fn data_resident_named(name: &'a str, data: Vec<u8>) -> Self {
        Self {
            attr_type: AttributeType::Data,
            content: NtfsAttributeContent::Resident(data),
            name,
            flags: 0,
        }
    }

    // ---------- Index Root ($INDEX_ROOT) ----------

    /// Build an $INDEX_ROOT for the given index name.
    ///
    /// `entries` must already contain the final "last entry" marker.
    pub fn index_root_named(
        name: &'a str,
        indexed_attr_type: u32,
        collation_rule: u32,
        entries: Vec<u8>,
        clusters_per_index_record: i8,
        index_record_size: u32,
        has_children: bool,
    ) -> Self {
        // INDEX_ROOT header
        let root = IndexRootHeader {
            indexed_attr_type,
            collation_rule,
            index_alloc_entry_size: index_record_size,
            clusters_per_index_record,
            padding: [0; 3],
        };

        // INDEX_NODE header: allocated_size should be >= index_length and typically aligned.
        // On fresh volumes, simplest safe choice is: allocated_size = index_length rounded up to 8.
        let index_len = (16 + entries.len()) as u32;
        let allocated = (index_len + 7) & !7;

        let node = IndexNodeHeader {
            entries_offset: 16,
            index_length: index_len,
            allocated_size: allocated,
            flags: if has_children { 1 } else { 0 },
            padding: [0; 3],
        };

        Self {
            attr_type: AttributeType::IndexRoot,
            content: NtfsAttributeContent::IndexRoot(root, node, entries),
            name,
            flags: 0,
        }
    }

    /// Convenience: directory root index ($I30) over $FILE_NAME collation.
    pub fn index_root_i30(
        entries: Vec<u8>,
        clusters_per_index_record: i8,
        index_record_size: u32,
        has_children: bool,
    ) -> Self {
        Self::index_root_named(
            "$I30",
            AttributeType::FileName.code(),
            1, // Collation: FileName
            entries,
            clusters_per_index_record,
            index_record_size,
            has_children,
        )
    }

    pub fn index_root_empty_i30(clusters_per_index_record: i8, index_record_size: u32) -> Self {
        let entries = Self::index_end_marker();
        Self::index_root_i30(entries, clusters_per_index_record, index_record_size, false)
    }

    #[inline]
    pub fn index_end_marker() -> Vec<u8> {
        // Standard "last entry" marker: 16 bytes.
        let mut m = vec![0u8; 16];
        m[8] = 0x10; // entry_length = 16
        m[12] = 0x02; // flags = LAST_ENTRY
        m
    }

    // ---------- $SECURITY_DESCRIPTOR ----------

    pub fn security_descriptor(descriptor: Vec<u8>) -> Self {
        Self {
            attr_type: AttributeType::SecurityDescriptor,
            content: NtfsAttributeContent::Resident(descriptor),
            name: "",
            flags: 0,
        }
    }

    // ---------- Non-resident builder (from RunList) ----------

    pub fn non_resident(
        attr_type: AttributeType,
        name: &'a str,
        meta: &NtfsMeta,
        runs: &rimio::run::RunList,
        data_size: u64,
    ) -> Self {
        let (dataruns, total_clusters) = Self::encode_runs(runs);
        let allocated_size = total_clusters * meta.bytes_per_cluster as u64;

        Self {
            attr_type,
            content: NtfsAttributeContent::NonResident {
                allocated_size,
                data_size,
                initialized_size: data_size,
                dataruns,
                lowest_vcn: 0,
                highest_vcn: total_clusters.saturating_sub(1),
            },
            name,
            flags: 0,
        }
    }

    #[inline]
    fn encode_runs(runs: &rimio::run::RunList) -> (Vec<u8>, u64) {
        let mut dataruns = Vec::new();
        let mut last_lcn = 0i64;
        let mut total_clusters = 0u64;

        for run in runs.iter() {
            let lcn_delta = run.start as i64 - last_lcn;
            let (encoded, len) = encode_data_run(lcn_delta, run.length);
            dataruns.extend_from_slice(&encoded[..len]);
            last_lcn = run.start as i64;
            total_clusters += run.length;
        }
        dataruns.push(encode_data_run_end());
        (dataruns, total_clusters)
    }

    // ---------- Bitmap / IndexAllocation wrappers ----------

    pub fn bitmap_named(name: &'a str, content: Vec<u8>) -> Self {
        Self {
            attr_type: AttributeType::Bitmap,
            content: NtfsAttributeContent::Resident(content),
            name,
            flags: 0,
        }
    }

    pub fn volume_info() -> Self {
        let info = VolumeInformation {
            reserved: 0,
            major_version: 3,
            minor_version: 1,
            flags: 0,
        };
        Self {
            attr_type: AttributeType::VolumeInformation,
            content: NtfsAttributeContent::Resident(info.as_bytes().to_vec()),
            name: "",
            flags: 0,
        }
    }

    pub fn volume_name(label_utf16: Vec<u16>) -> Self {
        Self {
            attr_type: AttributeType::VolumeName,
            content: NtfsAttributeContent::Resident(
                label_utf16.iter().flat_map(|&c| c.to_le_bytes()).collect(),
            ),
            name: "",
            flags: 0,
        }
    }
}
