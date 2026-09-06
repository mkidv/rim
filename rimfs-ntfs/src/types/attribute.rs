// SPDX-License-Identifier: MIT
//! NTFS Attribute Model
//!
//! Logical representation and builder helpers for resident and non-resident NTFS attributes.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use crate::attr::AttributeType;
use crate::flags::IndexEntryFlags;
use crate::meta::NtfsMeta;
use crate::types::{
    FileNameAttribute, IndexNodeHeader, IndexRootHeader, NtfsFileAttributes, NtfsFileNameNamespace,
    NtfsIndexEntry, StandardInformation, VolumeInformation,
};
use crate::utils::{current_ntfs_time, encode_data_run, encode_data_run_end};
use rimio::prelude::*;
use zerocopy::IntoBytes;

/// Logical representation of an Attribute
pub struct NtfsAttribute<'a> {
    pub attr_type: AttributeType,
    pub content: NtfsAttributeContent,
    pub name: &'a str, // Named attributes (ADS)
    pub flags: u16,
}

#[derive(Debug, Clone)]
pub enum NtfsAttributeContent {
    /// Resident content (raw bytes)
    Resident(Vec<u8>),
    /// Resident struct (Standard Information)
    StandardInformation(StandardInformation),
    /// Resident struct (File Name)
    FileName(FileNameAttribute, Vec<u16>), // Header + UTF-16 Name
    /// Non-Resident content
    NonResident {
        allocated_size: u64,
        data_size: u64,
        initialized_size: u64,
        dataruns: Vec<u8>, // Raw dataruns bytes
        lowest_vcn: u64,
        highest_vcn: u64,
    },
    /// Index Root (Directory)
    IndexRoot(IndexRootHeader, IndexNodeHeader, Vec<u8>), // Header + Node Header + Entries
}

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

    pub fn standard_info_basic(attrs: NtfsFileAttributes, timestamp: u64) -> Self {
        let mut data = vec![0u8; 48];
        data[0..8].copy_from_slice(&timestamp.to_le_bytes());
        data[8..16].copy_from_slice(&timestamp.to_le_bytes());
        data[16..24].copy_from_slice(&timestamp.to_le_bytes());
        data[24..32].copy_from_slice(&timestamp.to_le_bytes());
        data[32..36].copy_from_slice(&attrs.bits().to_le_bytes());
        Self {
            attr_type: AttributeType::StandardInformation,
            content: NtfsAttributeContent::Resident(data),
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
        Self::file_name_with_sizes_custom(
            parent_ref, name, data_size, data_size, attrs, namespace, timestamp,
        )
    }

    pub fn file_name_with_sizes(
        parent_ref: u64,
        name: &str,
        allocated_size: u64,
        data_size: u64,
        attrs: NtfsFileAttributes,
        namespace: NtfsFileNameNamespace,
    ) -> Self {
        Self::file_name_with_sizes_custom(
            parent_ref,
            name,
            allocated_size,
            data_size,
            attrs,
            namespace,
            Self::now(),
        )
    }

    pub fn file_name_with_sizes_custom(
        parent_ref: u64,
        name: &str,
        allocated_size: u64,
        data_size: u64,
        attrs: NtfsFileAttributes,
        namespace: NtfsFileNameNamespace,
        timestamp: u64,
    ) -> Self {
        let name_u16: Vec<u16> = name.encode_utf16().collect();
        let name_len = name_u16.len() as u8;

        let mut fn_attr =
            FileNameAttribute::new(parent_ref, data_size, attrs, name_len, namespace.bits());

        fn_attr.allocated_size = allocated_size;
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
        let root = IndexRootHeader {
            indexed_attr_type,
            collation_rule,
            index_alloc_entry_size: index_record_size,
            clusters_per_index_record,
            padding: [0; 3],
        };

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

    pub fn sparse_badclus(meta: &NtfsMeta) -> Self {
        let data_size = meta.total_clusters * meta.bytes_per_cluster as u64;
        Self {
            attr_type: AttributeType::Data,
            content: NtfsAttributeContent::NonResident {
                allocated_size: data_size,
                data_size,
                initialized_size: 0,
                dataruns: encode_sparse_run(meta.total_clusters),
                lowest_vcn: 0,
                highest_vcn: meta.total_clusters.saturating_sub(1),
            },
            name: "$Bad",
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

    pub fn as_i30_entry(&self, file_ref: u64) -> Option<NtfsIndexEntry> {
        let NtfsAttributeContent::FileName(file_name, name) = &self.content else {
            return None;
        };

        Some(
            NtfsIndexEntry::new(
                file_ref,
                file_name.parent_directory,
                name.clone(),
                NtfsFileAttributes::from_bits_retain(file_name.file_attributes),
                IndexEntryFlags::empty(),
                None,
            )
            .with_timestamps_raw(
                file_name.creation_time,
                file_name.modification_time,
                file_name.mft_modification_time,
                file_name.access_time,
            )
            .with_namespace(NtfsFileNameNamespace::from_raw(file_name.namespace))
            .with_sizes(file_name.data_size, file_name.allocated_size),
        )
    }

    pub fn write_to_io<IO: rimio::RimIO + ?Sized>(
        &self,
        io: &mut IO,
        offset: u64,
        attr_id: u16,
    ) -> rimio::prelude::RimIOResult<usize> {
        let name_bytes: Vec<u8> = self
            .name
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let name_len = (name_bytes.len() / 2) as u8;

        let (attr_header, resident, non_resident, content_bytes, dataruns) = match &self.content {
            NtfsAttributeContent::Resident(data) => {
                let resident = crate::types::ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 0,
                    padding: 0,
                };
                let header = crate::types::AttributeHeader {
                    attr_type: self.attr_type.code(),
                    length: 0,
                    non_resident: 0,
                    name_length: name_len,
                    name_offset: 0,
                    flags: self.flags,
                    attr_id,
                };
                (header, Some(resident), None, Some(data.as_slice()), None)
            }
            NtfsAttributeContent::StandardInformation(info) => {
                let data = info.as_bytes();
                let resident = crate::types::ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 0,
                    padding: 0,
                };
                let header = crate::types::AttributeHeader {
                    attr_type: self.attr_type.code(),
                    length: 0,
                    non_resident: 0,
                    name_length: name_len,
                    name_offset: 0,
                    flags: self.flags,
                    attr_id,
                };
                (header, Some(resident), None, Some(data), None)
            }
            NtfsAttributeContent::FileName(fn_attr, fn_name) => {
                let mut data = fn_attr.as_bytes().to_vec();
                for c in fn_name {
                    data.extend_from_slice(&c.to_le_bytes());
                }
                let resident = crate::types::ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 1,
                    padding: 0,
                };
                let header = crate::types::AttributeHeader {
                    attr_type: self.attr_type.code(),
                    length: 0,
                    non_resident: 0,
                    name_length: name_len,
                    name_offset: 0,
                    flags: self.flags,
                    attr_id,
                };

                return self.write_to_io_inner(
                    io,
                    offset,
                    attr_id,
                    header,
                    Some(resident),
                    None,
                    &data,
                    None,
                    &name_bytes,
                );
            }
            NtfsAttributeContent::NonResident {
                allocated_size,
                data_size,
                initialized_size,
                dataruns,
                lowest_vcn,
                highest_vcn,
            } => {
                let non_resident = crate::types::NonResidentAttributeHeader {
                    lowest_vcn: *lowest_vcn,
                    highest_vcn: *highest_vcn,
                    data_runs_offset: 0,
                    compression_unit: 0,
                    padding: 0,
                    allocated_size: *allocated_size,
                    data_size: *data_size,
                    initialized_size: *initialized_size,
                };
                let header = crate::types::AttributeHeader {
                    attr_type: self.attr_type.code(),
                    length: 0,
                    non_resident: 1,
                    name_length: name_len,
                    name_offset: 0,
                    flags: self.flags,
                    attr_id,
                };
                (
                    header,
                    None,
                    Some(non_resident),
                    None,
                    Some(dataruns.as_slice()),
                )
            }
            NtfsAttributeContent::IndexRoot(root, node, entries) => {
                let mut data = root.as_bytes().to_vec();
                data.extend_from_slice(node.as_bytes());
                data.extend_from_slice(entries);

                let resident = crate::types::ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 0,
                    padding: 0,
                };
                let header = crate::types::AttributeHeader {
                    attr_type: self.attr_type.code(),
                    length: 0,
                    non_resident: 0,
                    name_length: name_len,
                    name_offset: 0,
                    flags: self.flags,
                    attr_id,
                };
                return self.write_to_io_inner(
                    io,
                    offset,
                    attr_id,
                    header,
                    Some(resident),
                    None,
                    &data,
                    None,
                    &name_bytes,
                );
            }
        };

        if let Some(content) = content_bytes {
            self.write_to_io_inner(
                io,
                offset,
                attr_id,
                attr_header,
                resident,
                non_resident,
                content,
                dataruns,
                &name_bytes,
            )
        } else {
            self.write_to_io_inner(
                io,
                offset,
                attr_id,
                attr_header,
                resident,
                non_resident,
                &[],
                dataruns,
                &name_bytes,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn write_to_io_inner<IO: rimio::RimIO + ?Sized>(
        &self,
        io: &mut IO,
        offset: u64,
        _attr_id: u16,
        mut header: crate::types::AttributeHeader,
        resident: Option<crate::types::ResidentAttributeHeader>,
        non_resident: Option<crate::types::NonResidentAttributeHeader>,
        content: &[u8],
        dataruns: Option<&[u8]>,
        name_bytes: &[u8],
    ) -> rimio::prelude::RimIOResult<usize> {
        use crate::flags::AttributeFlags;
        use rimio::RimIOError;

        let extended_non_resident = header.non_resident != 0
            && (header.flags & (AttributeFlags::SPARSE | AttributeFlags::COMPRESSED).bits()) != 0;

        let header_len = if extended_non_resident {
            72
        } else if header.non_resident != 0 {
            64
        } else {
            24
        };

        let name_offset = header_len;
        let name_len_bytes = name_bytes.len();
        let content_offset = (name_offset + name_len_bytes + 7) & !7;

        if header.non_resident == 0 {
            let mut res =
                resident.ok_or(RimIOError::Invalid("Missing resident attribute header"))?;
            res.value_offset = content_offset as u16;

            let full_len = content_offset + content.len();
            let aligned_len = (full_len + 7) & !7;
            header.length = aligned_len as u32;
            header.name_offset = name_offset as u16;

            io.write_struct(offset, &header)?;
            io.write_struct(offset + 16, &res)?;

            if name_len_bytes > 0 {
                io.write_at(offset + name_offset as u64, name_bytes)?;
            }

            io.write_at(offset + content_offset as u64, content)?;

            let pad_len = aligned_len - full_len;
            if pad_len > 0 {
                io.write_at(offset + full_len as u64, &vec![0u8; pad_len])?;
            }
            Ok(aligned_len)
        } else {
            let mut non_res =
                non_resident.ok_or(RimIOError::Invalid("Missing non-resident attribute header"))?;
            let dr = dataruns.unwrap_or(&[]);
            non_res.data_runs_offset = content_offset as u16;

            let full_len = content_offset + dr.len();
            let aligned_len = (full_len + 7) & !7;
            header.length = aligned_len as u32;
            header.name_offset = name_offset as u16;

            io.write_struct(offset, &header)?;
            io.write_struct(offset + 16, &non_res)?;

            if extended_non_resident {
                io.write_at(offset + 64, &0u64.to_le_bytes())?;
            }

            if name_len_bytes > 0 {
                io.write_at(offset + name_offset as u64, name_bytes)?;
            }

            io.write_at(offset + content_offset as u64, dr)?;

            let pad_len = aligned_len - full_len;
            if pad_len > 0 {
                io.write_at(offset + full_len as u64, &vec![0u8; pad_len])?;
            }

            Ok(aligned_len)
        }
    }
}

fn encode_sparse_run(len_clusters: u64) -> Vec<u8> {
    let len_size = ((64 - len_clusters.leading_zeros()).max(1) as usize).div_ceil(8);

    let mut runs = Vec::with_capacity(len_size + 2);
    runs.push(len_size as u8);
    runs.extend_from_slice(&len_clusters.to_le_bytes()[..len_size]);
    runs.push(0);
    runs
}
