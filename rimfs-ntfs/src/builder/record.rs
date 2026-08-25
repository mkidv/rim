// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::attr::AttributeType;
use crate::types::{
    FileNameAttribute, IndexNodeHeader, IndexRootHeader, MftRecordHeader, StandardInformation,
};

/// Logical representation of an MFT Record
pub struct NtfsMftRecord<'a> {
    pub header: MftRecordHeader,
    pub attributes: Vec<NtfsAttribute<'a>>,
}

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
    /// Non-Resident content (simulated for now, usually just header setup)
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

impl<'a> NtfsMftRecord<'a> {
    pub fn new(record_number: u32, is_dir: bool, in_use: bool) -> Self {
        use crate::flags::MftRecordFlags;

        let mut flags = MftRecordFlags::empty();
        if in_use {
            flags |= MftRecordFlags::IN_USE;
        }
        if is_dir {
            flags |= MftRecordFlags::IS_DIRECTORY;
        }

        let mut header = MftRecordHeader::new(
            record_number,
            flags,
            1024, // Optimized default, will be updated during serialization
        );
        // Spec: "the sequence number for each of the system files is always equal to their mft record number"
        // This applies to the first 12 records (0-11).
        if record_number < 12 {
            // We use max(1) for record 0 to ensure a non-zero sequence number,
            // as seq=0 usually indicates a deleted/free record.
            header.sequence_number = record_number as u16;
        } else {
            // Records 12+ (including Extend children at 24+) start with sequence 1.
            header.sequence_number = 1;
        }

        Self {
            header,
            attributes: Vec::new(),
        }
    }

    pub fn add_attribute(&mut self, attr: NtfsAttribute<'a>) {
        self.attributes.push(attr);
        // Canonical NTFS Attribute Order:
        // 1. $STANDARD_INFORMATION (0x10)
        // 2. $ATTRIBUTE_LIST (0x20)
        // 3. $FILE_NAME (0x30)
        // 4. $OBJECT_ID (0x40)
        // 5. $SECURITY_DESCRIPTOR (0x50)
        // 6. $VOLUME_NAME (0x60)
        // 7. $VOLUME_INFORMATION (0x70)
        // 8. $DATA (0x80)
        // 9. $INDEX_ROOT (0x90)
        // 10. $INDEX_ALLOCATION (0xA0)
        // 11. $BITMAP (0xB0)
        // 12. $REPARSE_POINT (0xC0)
        // 13. $EA_INFORMATION (0xD0)
        // 14. $EA (0xE0)
        //
        // Sorting by Type Code (ascending), then by Name (ascending).
        // This matches Windows canonical behavior and prevents CHKDSK reordering.
        self.attributes.sort_by(|a, b| {
            let type_order = (a.attr_type as u32).cmp(&(b.attr_type as u32));
            if type_order == core::cmp::Ordering::Equal {
                a.name.cmp(b.name)
            } else {
                type_order
            }
        });
    }
}

impl<'a> NtfsAttribute<'a> {
    pub fn new(attr_type: AttributeType, content: NtfsAttributeContent, name: &'a str) -> Self {
        Self {
            attr_type,
            content,
            name,
            flags: 0,
        }
    }
}
