// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::FromBytes;

use crate::attr::{NtfsFileNameNamespace, current_ntfs_time};
use crate::flags::{IndexEntryFlags, NtfsFileAttributes};
use crate::meta::NtfsMeta;
use crate::types::{FileNameAttribute, IndexEntryHeader, IndexNodeHeader, IndexRecordHeader};
use crate::utils::{apply_usa_fixup, calculate_usa_size};
use rimio::prelude::*;

/// Logical representation of an Index Entry
#[derive(Debug, Clone)]
pub struct NtfsIndexEntry {
    pub file_ref: u64,
    pub parent_ref: u64,
    pub name: Vec<u16>,
    pub file_attr: NtfsFileAttributes,
    pub flags: IndexEntryFlags,
    pub vcn: Option<u64>,

    // New metadata for $FILE_NAME content robustness
    pub data_size: u64,
    pub allocated_size: u64,
    pub creation_time: u64,
    pub modification_time: u64,
    pub mft_modification_time: u64,
    pub access_time: u64,
    pub namespace: NtfsFileNameNamespace,

    pub raw: Option<Vec<u8>>, // Optional pre-built raw bytes (bypasses FileNameAttribute logic)
}

/// Logical representation of an Index Record (INDX block)
pub struct NtfsIndexRecord {
    pub vcn: u64,
    pub entries: Vec<NtfsIndexEntry>,
    pub has_children: bool,
}

impl NtfsIndexEntry {
    pub fn new(
        file_ref: u64,
        parent_ref: u64,
        name: Vec<u16>,
        file_attr: NtfsFileAttributes,
        flags: IndexEntryFlags,
        vcn: Option<u64>,
    ) -> Self {
        let now = current_ntfs_time();
        Self {
            file_ref,
            parent_ref,
            name,
            file_attr,
            flags,
            vcn,
            data_size: 0,
            allocated_size: 0,
            creation_time: now,
            modification_time: now,
            mft_modification_time: now,
            access_time: now,
            namespace: NtfsFileNameNamespace::Win32AndDos,
            raw: None,
        }
    }

    /// Builder methods for metadata
    pub fn with_sizes(mut self, data_size: u64, allocated_size: u64) -> Self {
        self.data_size = data_size;
        self.allocated_size = allocated_size;
        self
    }

    pub fn with_namespace(mut self, namespace: NtfsFileNameNamespace) -> Self {
        self.namespace = namespace;
        self
    }

    pub fn with_timestamps(mut self, time: u64) -> Self {
        self.creation_time = time;
        self.modification_time = time;
        self.mft_modification_time = time;
        self.access_time = time;
        self
    }

    pub fn len(&self) -> usize {
        if let Some(ref raw) = self.raw {
            return raw.len();
        }

        let is_last = self.flags.contains(IndexEntryFlags::LAST_ENTRY);
        let content_len = if is_last {
            0
        } else {
            core::mem::size_of::<FileNameAttribute>() + (self.name.len() * 2)
        };

        let mut entry_len = 16 + content_len;
        if self.vcn.is_some() || self.flags.contains(IndexEntryFlags::HAS_SUBNODES) {
            entry_len += 8;
        }
        (entry_len + 7) & !7 // Align 8
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }

    pub fn from_raw(raw: Vec<u8>) -> Self {
        // Create entry from raw bytes (used for sorting existing entries)
        let now = current_ntfs_time();
        Self {
            file_ref: 0,
            parent_ref: 0,
            name: Vec::new(),
            file_attr: NtfsFileAttributes::empty(),
            flags: IndexEntryFlags::empty(),
            vcn: None,
            data_size: 0,
            allocated_size: 0,
            creation_time: now,
            modification_time: now,
            mft_modification_time: now,
            access_time: now,
            namespace: NtfsFileNameNamespace::Win32AndDos,
            raw: Some(raw),
        }
    }

    pub fn name_from_raw(&self) -> Vec<u16> {
        if let Some(ref raw) = self.raw
            && raw.len() > 16
            && let Ok((fn_attr, _)) = FileNameAttribute::read_from_prefix(&raw[16..])
        {
            let name_len = fn_attr.filename_length as usize;
            let name_offset = 16 + core::mem::size_of::<FileNameAttribute>();
            if raw.len() >= name_offset + name_len * 2 {
                let name_bytes = &raw[name_offset..name_offset + name_len * 2];
                return name_bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
            }
        }
        self.name.clone()
    }

    /// Serialize this entry using RimIO
    pub fn write_to_io<IO: RimIO + ?Sized>(&self, io: &mut IO, offset: u64) -> RimIOResult<usize> {
        if let Some(ref raw) = self.raw {
            io.write_at(offset, raw)?;
            return Ok(raw.len());
        }

        let mut flags = self.flags;
        if self.vcn.is_some() {
            flags |= IndexEntryFlags::HAS_SUBNODES;
        }
        let is_last = flags.contains(IndexEntryFlags::LAST_ENTRY);

        let fn_attr_size = core::mem::size_of::<FileNameAttribute>();
        let name_bytes = self.name.len() * 2;
        let content_len = if is_last {
            0
        } else {
            fn_attr_size + name_bytes
        };

        // Align entry length to 8 bytes
        let mut entry_len = 16 + content_len;
        if flags.contains(IndexEntryFlags::HAS_SUBNODES) {
            entry_len += 8;
        }
        entry_len = (entry_len + 7) & !7;

        let header = IndexEntryHeader {
            mft_reference: self.file_ref,
            entry_length: entry_len as u16,
            content_length: content_len as u16,
            flags: flags.bits(),
            padding: [0; 3],
        };
        io.write_struct(offset, &header)?;

        if !is_last {
            let fn_attr = FileNameAttribute {
                parent_directory: self.parent_ref,
                creation_time: self.creation_time,
                modification_time: self.modification_time,
                mft_modification_time: self.mft_modification_time,
                access_time: self.access_time,
                allocated_size: self.allocated_size,
                data_size: self.data_size,
                file_attributes: self.file_attr.bits(),
                packed_ea_size: 0,
                reserved: 0,
                filename_length: self.name.len() as u8,
                namespace: self.namespace.bits(),
            };

            io.write_struct(offset + 16, &fn_attr)?;

            let mut name_buf = vec![0u8; name_bytes];
            for (i, &c) in self.name.iter().enumerate() {
                name_buf[i * 2] = c as u8;
                name_buf[i * 2 + 1] = (c >> 8) as u8;
            }
            io.write_at(offset + 16 + fn_attr_size as u64, &name_buf)?;
        }

        // Write VCN (at end)
        if flags.contains(IndexEntryFlags::HAS_SUBNODES) {
            let vcn_offset = entry_len - 8;
            let vcn_val = self.vcn.unwrap_or(0);
            io.write_at(offset + vcn_offset as u64, &vcn_val.to_le_bytes())?;
        }

        Ok(entry_len)
    }
}

impl NtfsIndexRecord {
    pub fn new(vcn: u64, has_children: bool) -> Self {
        Self {
            vcn,
            entries: Vec::new(),
            has_children,
        }
    }

    pub fn add_entry(&mut self, entry: NtfsIndexEntry) {
        self.entries.push(entry);
    }

    pub fn to_raw_buffer(&self, meta: &NtfsMeta) -> RimIOResult<Vec<u8>> {
        let record_size = meta.index_record_size as usize;
        let mut buf = vec![0u8; record_size];
        let mut io = rimio::prelude::MemRimIO::new(&mut buf);

        let usa_size = calculate_usa_size(meta.index_record_size, meta.bytes_per_sector as u32);

        let node_header_offset = 24u64;
        let usa_offset = 40u64;
        let entries_start = (usa_offset + (usa_size as u64 * 2) + 7) & !7;

        // 1. INDX Header
        let indx_header = IndexRecordHeader::new(self.vcn, usa_offset as u16, usa_size);
        io.write_struct(0, &indx_header)?;

        // 2. USA Source Array (Init to 0)
        // Update Sequence Number (USN) initialized to 1 usually
        let usa_len = usa_size as usize * 2;
        let mut usa = vec![0u8; usa_len];
        usa[0] = 1;
        io.write_at(usa_offset, &usa)?;

        // 3. Entries
        let mut pos = entries_start;
        for entry in &self.entries {
            if pos >= record_size as u64 {
                break;
            }
            let len = entry.write_to_io(&mut io, pos)?;
            pos += len as u64;
        }

        // Ensure End Entry
        let has_end = self
            .entries
            .last()
            .map(|e| e.flags.contains(IndexEntryFlags::LAST_ENTRY))
            .unwrap_or(false);

        if !has_end {
            let end = NtfsIndexEntry::new(
                0,
                0,
                Vec::new(),
                NtfsFileAttributes::empty(),
                IndexEntryFlags::LAST_ENTRY,
                None,
            );
            // Ignore error if full (should handle gracefully)
            if let Ok(len) = end.write_to_io(&mut io, pos) {
                pos += len as u64;
            }
        }

        // 4. Node Header
        let node_header = IndexNodeHeader {
            entries_offset: (entries_start - node_header_offset) as u32,
            index_length: (pos - node_header_offset) as u32,
            allocated_size: (record_size as u64 - node_header_offset) as u32,
            flags: if self.has_children { 1 } else { 0 },
            padding: [0; 3],
        };
        io.write_struct(node_header_offset, &node_header)?;

        // 5. Fixups
        apply_usa_fixup(&mut buf, meta.bytes_per_sector as usize);

        Ok(buf)
    }
}
