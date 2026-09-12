// SPDX-License-Identifier: MIT

//! NTFS B-tree index structures ($INDEX_ROOT, $INDEX_ALLOCATION, INDX records).

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::FromBytes;

use crate::attr::NtfsFileAttributes;
use crate::flags::IndexEntryFlags;
use crate::meta::NtfsMeta;
use crate::types::{
    FileNameAttribute, IndexEntryHeader, IndexNodeHeader, IndexRecordHeader, NtfsFileNameNamespace,
};
use crate::utils::*;
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

    pub fn with_timestamps_raw(
        mut self,
        creation_time: u64,
        modification_time: u64,
        mft_modification_time: u64,
        access_time: u64,
    ) -> Self {
        self.creation_time = creation_time;
        self.modification_time = modification_time;
        self.mft_modification_time = mft_modification_time;
        self.access_time = access_time;
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

    pub fn from_raw(raw: &[u8]) -> Option<Self> {
        if raw.len() < 16 {
            return None;
        }
        let (header, _) = IndexEntryHeader::ref_from_prefix(raw).ok()?;
        let mut flags = IndexEntryFlags::from_bits_truncate(header.flags);
        if flags.contains(IndexEntryFlags::LAST_ENTRY) {
            return None;
        }
        flags.remove(IndexEntryFlags::HAS_SUBNODES);

        let content_len = header.content_length.get() as usize;
        let fn_attr_size = core::mem::size_of::<FileNameAttribute>();
        if content_len < fn_attr_size || raw.len() < 16 + content_len {
            return None;
        }

        let fn_bytes = &raw[16..16 + content_len];
        let (fn_attr, _) = FileNameAttribute::ref_from_prefix(fn_bytes).ok()?;
        let name_len = fn_attr.filename_length as usize;
        let name_offset = fn_attr_size;
        if fn_bytes.len() < name_offset + name_len * 2 {
            return None;
        }
        let name_bytes = &fn_bytes[name_offset..name_offset + name_len * 2];
        let name: Vec<u16> = name_bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();

        let namespace = NtfsFileNameNamespace::from_raw(fn_attr.namespace);

        Some(Self {
            file_ref: header.mft_reference.get(),
            parent_ref: fn_attr.parent_directory.get(),
            name,
            file_attr: NtfsFileAttributes::from_bits_truncate(fn_attr.file_attributes.get()),
            flags,
            vcn: None,
            data_size: fn_attr.data_size.get(),
            allocated_size: fn_attr.allocated_size.get(),
            creation_time: fn_attr.creation_time.get(),
            modification_time: fn_attr.modification_time.get(),
            mft_modification_time: fn_attr.mft_modification_time.get(),
            access_time: fn_attr.access_time.get(),
            namespace,
            raw: None,
        })
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
            mft_reference: (self.file_ref).into(),
            entry_length: (entry_len as u16).into(),
            content_length: (content_len as u16).into(),
            flags: flags.bits(),
            padding: [0; 3],
        };
        io.write_struct(offset, &header)?;

        if !is_last {
            let fn_attr = FileNameAttribute {
                parent_directory: (self.parent_ref).into(),
                creation_time: (self.creation_time).into(),
                modification_time: (self.modification_time).into(),
                mft_modification_time: (self.mft_modification_time).into(),
                access_time: (self.access_time).into(),
                allocated_size: (self.allocated_size).into(),
                data_size: (self.data_size).into(),
                file_attributes: (self.file_attr.bits()).into(),
                packed_ea_size: (0).into(),
                reserved: (0).into(),
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
        let entries_start = 88u64.max((usa_offset + (usa_size as u64 * 2) + 7) & !7);

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
            entries_offset: ((entries_start - node_header_offset) as u32).into(),
            index_length: ((pos - node_header_offset) as u32).into(),
            allocated_size: ((record_size as u64 - node_header_offset) as u32).into(),
            flags: if self.has_children { 1 } else { 0 },
            padding: [0; 3],
        };
        io.write_struct(node_header_offset, &node_header)?;

        // 5. Fixups
        apply_usa_fixup(&mut buf, meta.bytes_per_sector as usize);

        Ok(buf)
    }
}

use crate::upcase::UpcaseHandle;
use zerocopy::IntoBytes;

pub struct IndexTreeLayout {
    pub root_entries: Vec<u8>,
    pub allocation_blocks: Vec<Vec<u8>>,
    pub bitmap: Vec<u8>,
    pub total_clusters: u64,
}

impl IndexTreeLayout {
    /// Writes all allocation blocks sequentially into the allocated disk runlist.
    pub fn write_allocation_blocks<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &NtfsMeta,
        runs: &rimio::run::RunList,
    ) -> RimIOResult {
        let mut mapped = MappedRimIO::new(io, runs, meta.bytes_per_cluster as usize);
        let mut off = 0u64;
        for block in &self.allocation_blocks {
            mapped.write_at(off, block)?;
            off += meta.index_record_size as u64;
        }
        Ok(())
    }
}

pub enum DirectoryIndexResult {
    /// Resident index root entries (small directory fitting in MFT record)
    Resident { entries_buf: Vec<u8> },
    /// Non-resident B-tree layout with allocation blocks and bitmap
    NonResident { layout: IndexTreeLayout },
}

pub struct IndexTreeBuilder;

impl IndexTreeBuilder {
    /// Build directory index: either resident buffer or non-resident tree layout.
    ///
    /// Automatically sorts entries according to NTFS UpCase collation rules
    /// and formats the terminator / subnodes as appropriate.
    pub fn build_directory_index(
        meta: &NtfsMeta,
        mut entries: Vec<NtfsIndexEntry>,
    ) -> RimIOResult<DirectoryIndexResult> {
        let upcase = UpcaseHandle::from_flavor(&meta.upcase_flavor);
        entries.sort_by(|a, b| compare_names_upcase(&a.name, &b.name, &upcase));

        let entries_len: usize = entries.iter().map(|e| e.len()).sum();
        let resident_max = meta.mft_record_size as usize / 3;

        if entries_len < resident_max {
            let mut buf = Vec::new();
            for e in &entries {
                let len = e.len();
                let old = buf.len();
                buf.resize(old + len, 0);
                let mut mem_io = MemRimIO::new(&mut buf[old..]);
                e.write_to_io(&mut mem_io, 0)?;
            }
            let last = IndexEntryHeader::new(0, 0, true);
            buf.extend_from_slice(last.as_bytes());
            Ok(DirectoryIndexResult::Resident { entries_buf: buf })
        } else {
            let layout = Self::build(meta, entries)?;
            Ok(DirectoryIndexResult::NonResident { layout })
        }
    }

    pub fn build(meta: &NtfsMeta, entries: Vec<NtfsIndexEntry>) -> RimIOResult<IndexTreeLayout> {
        let index_record_size = meta.index_record_size as usize;
        // Max payload in an Index Record (4KB usually)
        // Header (Indx + USA + padding) = 88 bytes
        // Node Header = 16 bytes
        // End Entry = 16 bytes
        // Safety margin = 32 bytes
        let max_payload = index_record_size.saturating_sub(88 + 32);

        let mut blocks: Vec<Vec<NtfsIndexEntry>> = Vec::new();
        let mut root_entries_indices: Vec<(usize, u64)> = Vec::new(); // (index in entries, vcn)

        let mut current_block_entries = Vec::new();
        let mut current_len = 0;

        // Partition entries into blocks
        for (i, entry) in entries.iter().enumerate() {
            let entry_len = entry.len();

            // Check if adding this entry + End Entry (16) overflows the block
            if current_len + entry_len + 16 > max_payload {
                let block_index = blocks.len() as u64;
                let vcn = meta.index_block_to_vcn(block_index);
                blocks.push(current_block_entries);

                // Promote current entry as pivot to the Root with child VCN
                root_entries_indices.push((i, vcn));

                current_block_entries = Vec::new();
                current_len = 0;
            } else {
                current_block_entries.push(entry.clone());
                current_len += entry_len;
            }
        }

        // Remaining entries go to last block
        let last_block_index = blocks.len() as u64;
        let last_block_vcn = meta.index_block_to_vcn(last_block_index);
        blocks.push(current_block_entries);

        let total_clusters = meta.total_clusters_for_index_blocks(blocks.len());

        let mut allocation_blocks = Vec::with_capacity(blocks.len());

        for (i, block_entries) in blocks.iter().enumerate() {
            let vcn = meta.index_block_to_vcn(i as u64);
            let mut record = NtfsIndexRecord::new(vcn, false);
            for entry in block_entries {
                record.add_entry(entry.clone());
            }
            allocation_blocks.push(record.to_raw_buffer(meta)?);
        }

        let mut root_entries_bytes = Vec::new();

        // Add pivot entries
        for (entry_idx, vcn) in root_entries_indices {
            let entry = &entries[entry_idx];
            let mut new_entry = entry.clone();
            new_entry.vcn = Some(vcn);
            new_entry.flags |= IndexEntryFlags::HAS_SUBNODES;

            let mut buf = vec![0u8; new_entry.len()];
            let mut io = MemRimIO::new(&mut buf);
            new_entry.write_to_io(&mut io, 0)?;
            root_entries_bytes.extend_from_slice(&buf);
        }

        // Add End Entry to Root (points to the last block)
        let mut last_entry = IndexEntryHeader::new(0, 0, true); // LAST_ENTRY
        last_entry.flags |= IndexEntryFlags::HAS_SUBNODES.bits();
        last_entry.entry_length = (last_entry.entry_length.get() + 8).into(); // +8 bytes for VCN

        let mut last_bytes = Vec::from(last_entry.as_bytes());
        last_bytes.extend_from_slice(&last_block_vcn.to_le_bytes());
        root_entries_bytes.extend_from_slice(&last_bytes);

        // Windows/mkntfs stores the $I30 bitmap with a minimum size of 8 bytes.
        let bitmap_len = blocks.len().div_ceil(8).max(8);
        let mut bitmap = vec![0u8; bitmap_len];

        for i in 0..blocks.len() {
            bitmap[i / 8] |= 1 << (i % 8);
        }

        Ok(IndexTreeLayout {
            root_entries: root_entries_bytes,
            allocation_blocks,
            bitmap,
            total_clusters,
        })
    }
}
