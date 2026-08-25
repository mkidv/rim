// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::builder::{NtfsAttribute, NtfsAttributeContent, NtfsMftRecord};
use crate::meta::NtfsMeta;
use crate::types::{
    AttributeHeader, MftRecordHeader, NonResidentAttributeHeader, ResidentAttributeHeader,
};

/// Helper to build MFT records declaratively
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
        // Ensure 8-byte alignment
        self.next_offset = (self.next_offset + 7) & !7;

        let attr_id = self.next_attr_id;
        self.next_attr_id += 1;

        let bytes_written =
            attr.write_to_io(self.io, self.record_start + self.next_offset, attr_id)?;
        self.next_offset += bytes_written as u64;

        Ok(())
    }

    pub fn finalize(&mut self) -> RimIOResult<u32> {
        // Align and write End Marker
        self.next_offset = (self.next_offset + 7) & !7;
        let end_marker: u32 = 0xFFFFFFFF;
        self.io.write_at(
            self.record_start + self.next_offset,
            &end_marker.to_le_bytes(),
        )?;
        self.next_offset += 8; // standard alignment

        let used = self.next_offset as u32;

        // Update header with real used bytes
        let mut header: MftRecordHeader = self.io.read_struct(self.record_start)?;
        header.bytes_used = used;
        header.next_attr_id = self.next_attr_id;
        self.io.write_struct(self.record_start, &header)?;

        // Apply Fixups
        let record_size = self.meta.mft_record_size as usize;
        let mut buf = vec![0u8; record_size];
        self.io.read_at(self.record_start, &mut buf)?;

        // Update sequence array (standard initialization)
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

impl<'a> NtfsMftRecord<'a> {
    /// Serialize the record to a raw buffer
    pub fn to_raw_buffer(&self, meta: &NtfsMeta) -> RimIOResult<Vec<u8>> {
        let mut buf = vec![0u8; meta.mft_record_size as usize];
        {
            let mut mem_io = rimio::prelude::MemRimIO::new(&mut buf);
            let mut builder = MftRecordBuilder::new(&mut mem_io, meta, 0);
            builder.write_header(self.header)?;

            for attr in &self.attributes {
                builder.write_attribute(attr)?;
            }

            builder.finalize()?;
        }
        Ok(buf)
    }
}

impl<'a> NtfsAttribute<'a> {
    pub fn write_to_io<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        offset: u64,
        attr_id: u16,
    ) -> RimIOResult<usize> {
        let name_bytes: Vec<u8> = self
            .name
            .encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        let name_len = (name_bytes.len() / 2) as u8;

        let (attr_header, resident, non_resident, content_bytes, dataruns) = match &self.content {
            NtfsAttributeContent::Resident(data) => {
                let resident = ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0, // Set later
                    indexed: 0,
                    padding: 0,
                };
                let header = AttributeHeader {
                    attr_type: self.attr_type.code(),
                    length: 0, // Set later
                    non_resident: 0,
                    name_length: name_len,
                    name_offset: 0, // Set later
                    flags: self.flags,
                    attr_id,
                };
                (header, Some(resident), None, Some(data.as_slice()), None)
            }
            NtfsAttributeContent::StandardInformation(info) => {
                let data = info.as_bytes();
                let resident = ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 0,
                    padding: 0,
                };
                let header = AttributeHeader {
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
                let resident = ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 1, // FileNames are indexed
                    padding: 0,
                };
                let header = AttributeHeader {
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
                let non_resident = NonResidentAttributeHeader {
                    lowest_vcn: *lowest_vcn,
                    highest_vcn: *highest_vcn,
                    data_runs_offset: 0, // Set later
                    compression_unit: 0,
                    padding: 0,
                    allocated_size: *allocated_size,
                    data_size: *data_size,
                    initialized_size: *initialized_size,
                };
                let header = AttributeHeader {
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

                let resident = ResidentAttributeHeader {
                    value_length: data.len() as u32,
                    value_offset: 0,
                    indexed: 0,
                    padding: 0,
                };
                let header = AttributeHeader {
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
            // Non-resident case fallback
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
    fn write_to_io_inner<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        offset: u64,
        _attr_id: u16,
        mut header: AttributeHeader,
        resident: Option<ResidentAttributeHeader>,
        non_resident: Option<NonResidentAttributeHeader>,
        content: &[u8],
        dataruns: Option<&[u8]>,
        name_bytes: &[u8],
    ) -> RimIOResult<usize> {
        let header_len = if header.non_resident != 0 { 64 } else { 24 }; // 16 + 48 or 16 + 8

        let name_offset = header_len;
        let name_len_bytes = name_bytes.len();

        let content_offset = (name_offset + name_len_bytes + 7) & !7;

        if header.non_resident == 0 {
            // Resident
            let mut res =
                resident.ok_or(RimIOError::Invalid("Missing resident attribute header"))?;
            res.value_offset = content_offset as u16;

            let full_len = content_offset + content.len();
            let aligned_len = (full_len + 7) & !7;
            header.length = aligned_len as u32;
            header.name_offset = if name_len_bytes > 0 {
                name_offset as u16
            } else {
                0
            };

            io.write_struct(offset, &header)?;
            io.write_struct(offset + 16, &res)?;

            if name_len_bytes > 0 {
                io.write_at(offset + name_offset as u64, name_bytes)?;
            }

            io.write_at(offset + content_offset as u64, content)?;

            // Zero padding
            let pad_len = aligned_len - full_len;
            if pad_len > 0 {
                io.write_at(offset + full_len as u64, &vec![0u8; pad_len])?;
            }
            Ok(aligned_len)
        } else {
            // Non-Resident
            let mut non_res =
                non_resident.ok_or(RimIOError::Invalid("Missing non-resident attribute header"))?;
            let dr = dataruns.unwrap_or(&[]);
            non_res.data_runs_offset = content_offset as u16;

            let full_len = content_offset + dr.len();
            let aligned_len = (full_len + 7) & !7;
            header.length = aligned_len as u32;
            header.name_offset = if name_len_bytes > 0 {
                name_offset as u16
            } else {
                0
            };

            io.write_struct(offset, &header)?;
            io.write_struct(offset + 16, &non_res)?;

            if name_len_bytes > 0 {
                io.write_at(offset + name_offset as u64, name_bytes)?;
            }

            io.write_at(offset + content_offset as u64, dr)?;

            // Zero padding
            let pad_len = aligned_len - full_len;
            if pad_len > 0 {
                io.write_at(offset + full_len as u64, &vec![0u8; pad_len])?;
            }
            Ok(aligned_len)
        }
    }
}
