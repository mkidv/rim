// SPDX-License-Identifier: MIT
//! NTFS Master File Table (MFT) Record Model
//!
//! Logical representation and serialization of MFT file records with USA/MST fixups.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::constant::*;
use crate::flags::{MftRecordFlags, NtfsFileAttributes};
use crate::meta::NtfsMeta;
use crate::types::attribute::{NtfsAttribute, NtfsAttributeContent};
use crate::types::{MftRecordHeader, NtfsIndexEntry};
use crate::utils::build_mft_reference;

/// Logical representation of an MFT Record
pub struct NtfsMftRecord<'a> {
    pub header: MftRecordHeader,
    pub attributes: Vec<NtfsAttribute<'a>>,
}

impl<'a> NtfsMftRecord<'a> {
    pub fn new(record_number: u32, is_dir: bool, in_use: bool) -> Self {
        let mut flags = MftRecordFlags::empty();
        if in_use {
            flags |= MftRecordFlags::IN_USE;
        }
        if is_dir {
            flags |= MftRecordFlags::IS_DIRECTORY;
        }

        let mut header = MftRecordHeader::new(record_number, flags, 1024);
        // System/reserved records 0-15 use their MFT record number as sequence
        // number, with record 0 using sequence number 1.
        if record_number < MFT_RECORD_FREE_START as u32 {
            header.sequence_number = match record_number {
                0 | 1 => 1,
                2..=15 => record_number as u16,
                _ => 1,
            };
        } else {
            header.sequence_number = 1;
        }

        Self {
            header,
            attributes: Vec::new(),
        }
    }

    pub fn add_attribute(&mut self, attr: NtfsAttribute<'a>) {
        self.attributes.push(attr);
    }

    /// Serialize the record to a raw buffer with MST / USA fixups applied.
    pub fn to_raw_buffer(&self, meta: &NtfsMeta) -> RimIOResult<Vec<u8>> {
        let mut buf = vec![0u8; meta.mft_record_size as usize];
        {
            let mut mem_io = MemRimIO::new(&mut buf);
            let mut builder = MftRecordBuilder::new(&mut mem_io, meta, 0);
            builder.write_header(self.header)?;

            for attr in &self.attributes {
                builder.write_attribute(attr)?;
            }

            builder.finalize()?;
        }
        Ok(buf)
    }

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
        let file_name_attrs = if attrs.contains(NtfsFileAttributes::DIRECTORY) {
            (attrs - NtfsFileAttributes::DIRECTORY) | NtfsFileAttributes::I30_INDEX
        } else {
            attrs
        };

        record.add_attribute(NtfsAttribute::file_name(
            parent_ref,
            name,
            0,
            file_name_attrs,
            NtfsFileNameNamespace::Win32AndDos,
        ));

        let clusters_per_index = meta.clusters_per_index_record_raw();

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

    pub fn i30_entry(&self) -> Option<NtfsIndexEntry> {
        let file_ref = build_mft_reference(
            self.header.mft_record_number as u64,
            self.header.sequence_number,
        );

        self.attributes
            .iter()
            .find(|attr| attr.attr_type == AttributeType::FileName)?
            .as_i30_entry(file_ref)
    }
}

/// Helper to serialize MFT records declaratively into an IO stream
pub struct MftRecordBuilder<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a NtfsMeta,
    record_start: u64,
    next_offset: u64,
    next_attr_id: u16,
}

impl<'a, IO: RimIO + ?Sized> MftRecordBuilder<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta, record_start: u64) -> Self {
        Self {
            io,
            meta,
            record_start,
            next_offset: 0,
            next_attr_id: 1,
        }
    }

    pub fn write_header(&mut self, mut header: MftRecordHeader) -> RimIOResult {
        let sector_size = self.meta.bytes_per_sector as u64;
        let record_size = self.meta.mft_record_size as u64;
        let usa_offset = 48u64;
        let usa_count = (record_size / sector_size) + 1;

        header.usa_offset = usa_offset as u16;
        header.usa_count = usa_count as u16;
        header.bytes_allocated = record_size as u32;

        let usa_size = usa_count * 2;
        let attrs_offset = (usa_offset + usa_size + 7) & !7;
        header.attrs_offset = attrs_offset as u16;

        self.io.write_struct(self.record_start, &header)?;
        self.next_offset = attrs_offset;
        Ok(())
    }

    pub fn write_attribute(&mut self, attr: &NtfsAttribute) -> RimIOResult {
        self.next_offset = (self.next_offset + 7) & !7;

        let attr_id = self.next_attr_id;
        self.next_attr_id += 1;

        let bytes_written =
            attr.write_to_io(self.io, self.record_start + self.next_offset, attr_id)?;
        self.next_offset += bytes_written as u64;

        Ok(())
    }

    pub fn finalize(&mut self) -> RimIOResult<u32> {
        self.next_offset = (self.next_offset + 7) & !7;
        let end_marker: u32 = 0xFFFFFFFF;
        self.io.write_at(
            self.record_start + self.next_offset,
            &end_marker.to_le_bytes(),
        )?;
        self.next_offset += 8;

        let used = self.next_offset as u32;

        let mut header: MftRecordHeader = self.io.read_struct(self.record_start)?;
        header.bytes_used = used;
        header.next_attr_id = self.next_attr_id;
        self.io.write_struct(self.record_start, &header)?;

        let record_size = self.meta.mft_record_size as usize;
        let mut buf = vec![0u8; record_size];
        self.io.read_at(self.record_start, &mut buf)?;

        let usa_offset = header.usa_offset as usize;
        let usa_count = header.usa_count as usize;
        let usa_num = 1u16;
        buf[usa_offset..usa_offset + 2].copy_from_slice(&usa_num.to_le_bytes());
        for i in 1..usa_count {
            buf[usa_offset + i * 2..usa_offset + (i + 1) * 2].copy_from_slice(&[0, 0]);
        }

        crate::utils::apply_usa_fixup(&mut buf, self.meta.bytes_per_sector as usize);
        self.io.write_at(self.record_start, &buf)?;

        Ok(used)
    }
}
