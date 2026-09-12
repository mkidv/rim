// SPDX-License-Identifier: MIT
//! NTFS Master File Table (MFT) Record Model
//!
//! Logical representation and serialization of MFT file records with USA/MST fixups.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::attr::NtfsFileAttributes;
use crate::flags::MftRecordFlags;
use crate::meta::NtfsMeta;
use crate::mft::build_mft_reference;
use crate::types::attribute::{NtfsAttribute, NtfsAttributeContent};
use crate::types::{MftRecordHeader, NtfsAttributeType, NtfsFileNameNamespace, NtfsIndexEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum NtfsMftRecordNumber {
    // System file MFT record numbers
    Mft = 0,
    MftMirr = 1,
    LogFile = 2,
    Volume = 3,
    AttrDef = 4,
    Root = 5,
    Bitmap = 6,
    Boot = 7,
    BadClus = 8,
    Secure = 9,
    Upcase = 10,
    Extend = 11,
    // Reserved system records (12-15 are in-use but empty, 16-23 are free)
    // $Extend children (assigned to > 24 according to specs)
    Quota = 24,
    ObjId = 25,
    Reparse = 26,
    UsnJrnl = 27,
}

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
        if record_number < 16_u32 {
            header.sequence_number = (match record_number {
                0 | 1 => 1,
                2..=15 => record_number as u16,
                _ => 1,
            })
            .into();
        } else {
            header.sequence_number = (1).into();
        }

        Self {
            header,
            attributes: Vec::new(),
        }
    }

    pub fn add_attribute(&mut self, attr: NtfsAttribute<'a>) {
        self.attributes.push(attr);
    }

    /// Serialize the record into an existing mutable byte buffer with MST / USA fixups applied.
    pub fn serialize_into(&self, buf: &mut [u8], meta: &NtfsMeta) -> RimIOResult {
        let record_size = meta.mft_record_size as usize;
        let sector_size = meta.bytes_per_sector as usize;
        if sector_size < 2
            || record_size < sector_size
            || !record_size.is_multiple_of(sector_size)
            || buf.len() < record_size
        {
            return Err(RimIOError::Invalid("Invalid MFT buffer or geometry"));
        }
        let buf = &mut buf[..record_size];
        buf.fill(0);
        let header = {
            let mut mem_io = MemRimIO::new(buf);
            let mut builder = MftRecordBuilder::new(&mut mem_io, meta, 0);
            builder.write_header(self.header)?;
            for attr in &self.attributes {
                builder.write_attribute(attr)?;
            }
            builder.finalize_header()?
        };
        initialize_record_fixup(buf, &header, sector_size)?;
        Ok(())
    }

    /// Serialize the record to a raw buffer with MST / USA fixups applied.
    pub fn to_raw_buffer(&self, meta: &NtfsMeta) -> RimIOResult<Vec<u8>> {
        let mut buf = vec![0u8; meta.mft_record_size as usize];
        self.serialize_into(&mut buf, meta)?;
        Ok(buf)
    }

    /// Write record directly to the MFT at record_number without heap allocation when record_size == 1024.
    pub fn write_to_mft<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &NtfsMeta,
        record_number: u64,
    ) -> RimIOResult {
        let offset = meta.mft_record_offset(record_number);
        if meta.mft_record_size == 1024 {
            let mut buf = [0u8; 1024];
            self.serialize_into(&mut buf, meta)?;
            io.write_at(offset, &buf)
        } else {
            let raw = self.to_raw_buffer(meta)?;
            io.write_at(offset, &raw)
        }
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
            attr_type: NtfsAttributeType::Data,
            content,
            name: "",
            flags: 0,
        });

        record
    }

    pub fn i30_entry(&self) -> Option<NtfsIndexEntry> {
        let file_ref = build_mft_reference(
            self.header.mft_record_number.get() as u64,
            self.header.sequence_number.get(),
        );

        self.attributes
            .iter()
            .find(|attr| attr.attr_type == NtfsAttributeType::FileName)?
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
        if sector_size < 2
            || record_size < sector_size
            || !record_size.is_multiple_of(sector_size)
            || record_size / sector_size >= u16::MAX as u64
        {
            return Err(RimIOError::Invalid("Invalid MFT record geometry"));
        }
        let usa_offset = 48u64;
        let usa_count = (record_size / sector_size) + 1;

        header.usa_offset = (usa_offset as u16).into();
        header.usa_count = (usa_count as u16).into();
        header.bytes_allocated = (record_size as u32).into();

        let usa_size = usa_count * 2;
        let attrs_offset = (usa_offset + usa_size + 7) & !7;
        if attrs_offset > u16::MAX as u64 || attrs_offset + 8 > record_size {
            return Err(RimIOError::Invalid("MFT USA leaves no attribute space"));
        }
        header.attrs_offset = (attrs_offset as u16).into();

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

    fn finalize_header(&mut self) -> RimIOResult<MftRecordHeader> {
        self.next_offset = (self.next_offset + 7) & !7;
        if self
            .next_offset
            .checked_add(8)
            .is_none_or(|end| end > self.meta.mft_record_size as u64)
        {
            return Err(RimIOError::Invalid("MFT attributes exceed record capacity"));
        }
        let end_marker: u32 = 0xFFFFFFFF;
        self.io.write_at(
            self.record_start + self.next_offset,
            &end_marker.to_le_bytes(),
        )?;
        self.next_offset += 8;

        let used = self.next_offset as u32;

        let mut header: MftRecordHeader = self.io.read_struct(self.record_start)?;
        header.bytes_used = (used).into();
        header.next_attr_id = (self.next_attr_id).into();
        self.io.write_struct(self.record_start, &header)?;

        Ok(header)
    }

    pub fn finalize(&mut self) -> RimIOResult<u32> {
        let header = self.finalize_header()?;
        let record_size = self.meta.mft_record_size as usize;
        if record_size == 1024 {
            let mut buf = [0u8; 1024];
            self.io.read_at(self.record_start, &mut buf)?;
            initialize_record_fixup(&mut buf, &header, self.meta.bytes_per_sector as usize)?;
            self.io.write_at(self.record_start, &buf)?;
        } else {
            let mut buf = vec![0u8; record_size];
            self.io.read_at(self.record_start, &mut buf)?;
            initialize_record_fixup(&mut buf, &header, self.meta.bytes_per_sector as usize)?;
            self.io.write_at(self.record_start, &buf)?;
        }

        Ok(header.bytes_used.get())
    }
}

fn initialize_record_fixup(
    buf: &mut [u8],
    header: &MftRecordHeader,
    sector_size: usize,
) -> RimIOResult {
    let offset = header.usa_offset.get() as usize;
    let count = header.usa_count.get() as usize;
    if sector_size < 2
        || count < 2
        || offset + count * 2 > buf.len()
        || (count - 1) * sector_size != buf.len()
    {
        return Err(RimIOError::Invalid("Invalid MFT USA geometry"));
    }
    buf[offset..offset + 2].copy_from_slice(&1u16.to_le_bytes());
    buf[offset + 2..offset + count * 2].fill(0);
    crate::utils::apply_usa_fixup(buf, sector_size);
    Ok(())
}

#[cfg(test)]
mod serialization_tests {
    use super::*;
    #[test]
    fn direct_serialization_matches_generic_builder_and_preserves_tail() {
        for (record_size, sector_size) in [(1024, 512), (4096, 512), (4096, 4096)] {
            let mut meta = NtfsMeta::new(64 * 1024 * 1024, None).unwrap();
            meta.mft_record_size = record_size;
            meta.bytes_per_sector = sector_size;
            let record = NtfsMftRecord::new(42, false, true);
            let mut old = vec![0; record_size as usize];
            {
                let mut io = MemRimIO::new(&mut old);
                let mut builder = MftRecordBuilder::new(&mut io, &meta, 0);
                builder.write_header(record.header).unwrap();
                builder.finalize().unwrap();
            }
            let mut direct = vec![0xaa; record_size as usize + 7];
            record.serialize_into(&mut direct, &meta).unwrap();
            assert_eq!(&direct[..record_size as usize], old);
            assert_eq!(&direct[record_size as usize..], &[0xaa; 7]);
            assert!(
                record
                    .serialize_into(&mut direct[..record_size as usize - 1], &meta)
                    .is_err()
            );
            assert!(crate::utils::decode_usa_fixup(
                &mut direct[..record_size as usize],
                meta.bytes_per_sector as usize
            ));
        }
    }
}
