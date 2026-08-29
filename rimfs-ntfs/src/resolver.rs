// SPDX-License-Identifier: MIT
//! NTFS filesystem resolver (parser)
//!
//! Responsible for reading and traversing an existing NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::RimIO;
use zerocopy::FromBytes;

use crate::constant::*;
use crate::core::errors::{FsResolverError, FsResolverResult};
use crate::core::resolver::FsTreeResolver;
use crate::core::resolver::attr::FileAttributes;
use crate::flags::*;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::*;
use crate::upcase::UpcaseHandle;
use crate::view::attr_view::{AttrRef, AttrView};
use crate::view::mft_view::MftRecordView;

pub struct NtfsResolver<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a NtfsMeta,
}

impl<'a, IO: RimIO + ?Sized> NtfsResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> Self {
        Self { io, meta }
    }

    /// Read an MFT record by its record number
    pub fn read_mft_record(&mut self, record_number: u64) -> FsResolverResult<Vec<u8>> {
        let record = mft::read_record(self.io, self.meta, record_number)?;
        Ok(record)
    }

    /// Find an attribute in an MFT record buffer
    pub fn find_attribute<'b>(
        &self,
        record: &'b [u8],
        attr_type: u32,
    ) -> FsResolverResult<Option<AttrRef<'b>>> {
        self.find_attribute_named(record, attr_type, None)
    }

    /// Find an attribute in an MFT record buffer with optional stream name
    pub fn find_attribute_named<'b>(
        &self,
        record: &'b [u8],
        attr_type: u32,
        stream_name: Option<&str>,
    ) -> FsResolverResult<Option<AttrRef<'b>>> {
        let view = MftRecordView::new(record)
            .map_err(|_| FsResolverError::Invalid("Failed to read MFT header"))?;

        view.find_named(attr_type, stream_name)
            .map_err(|_| FsResolverError::Invalid("Malformed attribute"))
    }

    /// Read data of a specific stream (or unnamed default stream if None) from an MFT record
    pub fn read_file_stream(
        &mut self,
        record_number: u64,
        stream_name: Option<&str>,
    ) -> FsResolverResult<Vec<u8>> {
        let record = self.read_mft_record(record_number)?;
        let attr = self
            .find_attribute_named(&record, ATTR_DATA, stream_name)?
            .ok_or(FsResolverError::NotFound)?;

        if attr.is_resident() {
            let content = self.get_resident_attribute_content(attr)?;
            Ok(content.to_vec())
        } else {
            self.get_non_resident_attribute_content(attr)
        }
    }

    /// Get the content of a resident attribute
    pub fn get_resident_attribute_content<'b>(
        &self,
        attr: AttrRef<'b>,
    ) -> FsResolverResult<&'b [u8]> {
        let view = attr
            .as_view()
            .map_err(|_| FsResolverError::Invalid("Malformed attribute"))?;

        match view {
            AttrView::Resident { value, .. } => Ok(value),
            AttrView::NonResident { .. } => {
                Err(FsResolverError::Invalid("Attribute is not resident"))
            }
        }
    }

    /// Get content of a non-resident attribute
    pub fn get_non_resident_attribute_content(
        &mut self,
        attr: AttrRef,
    ) -> FsResolverResult<Vec<u8>> {
        let view = attr
            .as_view()
            .map_err(|_| FsResolverError::Invalid("Malformed attribute"))?;

        let (runlist, data_size) = match view {
            AttrView::NonResident {
                runlist, data_size, ..
            } => (runlist, data_size),
            AttrView::Resident { .. } => {
                return Err(FsResolverError::Invalid("Attribute is resident"));
            }
        };

        let mut content = Vec::with_capacity(data_size as usize);
        let cluster_size = self.meta.bytes_per_cluster as usize;

        for run in runlist.iter() {
            if content.len() as u64 >= data_size {
                break;
            }

            match run.lcn {
                Some(lcn) => {
                    let offset = self.meta.lcn_to_offset(lcn);
                    let size = (run.len as usize) * cluster_size;

                    let mut buf = vec![0u8; size];
                    self.io
                        .read_at(offset, &mut buf)
                        .map_err(FsResolverError::IO)?;

                    content.extend_from_slice(&buf);
                }
                None => {
                    // Sparse run
                    let size = (run.len as usize) * cluster_size;
                    content.resize(content.len() + size, 0);
                }
            }
        }

        content.truncate(data_size as usize);
        Ok(content)
    }

    /// Read the index root of a directory
    pub(crate) fn _read_index_root(&self, record: &[u8]) -> FsResolverResult<Vec<u8>> {
        let attr = self
            .find_attribute(record, ATTR_INDEX_ROOT)?
            .ok_or(FsResolverError::Invalid("Directory has no $INDEX_ROOT"))?;

        let content = self.get_resident_attribute_content(attr)?;
        Ok(content.to_vec())
    }

    /// Read entries from a directory MFT record
    ///
    /// Supports resident indexes in $INDEX_ROOT and non-resident in $INDEX_ALLOCATION
    pub fn read_directory_entries(
        &mut self,
        record_number: u64,
    ) -> FsResolverResult<Vec<(String, u64, FileAttributes)>> {
        let record = self.read_mft_record(record_number)?;

        let header = MftRecordHeader::read_from_prefix(&record)
            .map_err(|_| FsResolverError::Invalid("Failed to read MFT header"))?
            .0;

        if !header.is_dir() {
            return Err(FsResolverError::Invalid("Not a directory"));
        }

        let mut entries = Vec::new();

        // 1. Read Resident $INDEX_ROOT ($I30 or unnamed)
        let index_root_attr =
            match self.find_attribute_named(&record, ATTR_INDEX_ROOT, Some("$I30"))? {
                Some(a) => Some(a),
                None => self.find_attribute(&record, ATTR_INDEX_ROOT)?,
            };

        if let Some(attr) = index_root_attr {
            let content = self.get_resident_attribute_content(attr)?;

            // Parse Index Root Header (16 bytes)
            let _root_header = IndexRootHeader::read_from_prefix(content)
                .map_err(|_| FsResolverError::Invalid("Failed to read Index Root Header"))?
                .0;

            let node_header_offset = core::mem::size_of::<IndexRootHeader>();
            if node_header_offset < content.len() {
                self.parse_entries_from_node_header(&content[node_header_offset..], &mut entries)?;
            }
        }

        // 2. Read Non-Resident $INDEX_ALLOCATION ($I30 or unnamed)
        let index_alloc_attr =
            match self.find_attribute_named(&record, ATTR_INDEX_ALLOCATION, Some("$I30"))? {
                Some(a) => Some(a),
                None => self.find_attribute(&record, ATTR_INDEX_ALLOCATION)?,
            };

        if let Some(attr) = index_alloc_attr {
            // Check if resident or non-resident (Index Allocation is typically non-resident)
            let content = if let Ok(res) = self.get_resident_attribute_content(attr) {
                res.to_vec()
            } else {
                self.get_non_resident_attribute_content(attr)?
            };

            // Iterate over blocks (Index Records)
            let block_size = self.meta.index_record_size as usize;
            for chunk in content.chunks(block_size) {
                if chunk.len() < block_size {
                    continue;
                }

                // Verify "INDX" signature
                if &chunk[0..4] != b"INDX" {
                    continue; // Unallocated or invalid block
                }

                // Decode USA fixup
                let mut block = chunk.to_vec();
                if !crate::utils::decode_usa_fixup(&mut block, self.meta.bytes_per_sector as usize)
                {
                    continue; // Invalid USA fixup
                }

                // In standard NTFS INDX records, IndexNodeHeader is at offset 24 (immediately after IndexRecordHeader)
                let node_header_offset = core::mem::size_of::<IndexRecordHeader>();
                if node_header_offset < block.len() {
                    self.parse_entries_from_node_header(
                        &block[node_header_offset..],
                        &mut entries,
                    )?;
                }
            }
        }

        Ok(entries)
    }

    fn parse_entries_from_node_header(
        &self,
        buf: &[u8],
        entries: &mut Vec<(String, u64, FileAttributes)>,
    ) -> FsResolverResult<()> {
        let node_header = IndexNodeHeader::read_from_prefix(buf)
            .map_err(|_| FsResolverError::Invalid("Failed to read Index Node Header"))?
            .0;

        let start_offset = node_header.entries_offset as usize;
        let end_offset = node_header.index_length as usize; // Total used size

        // Bounds check
        if start_offset >= buf.len() || end_offset > buf.len() {
            return Ok(());
        }

        let mut offset = start_offset;

        while offset < end_offset {
            if offset + 16 > buf.len() {
                break;
            }

            let entry_header = IndexEntryHeader::read_from_prefix(&buf[offset..])
                .map_err(|_| FsResolverError::Invalid("Failed to read Index Entry Header"))?
                .0;

            if entry_header.entry_length < 16 {
                break;
            }

            // Last entry marker
            if (entry_header.flags & IndexEntryFlags::LAST_ENTRY.bits()) != 0 {
                break;
            }

            // Extract filename attribute body
            let content_offset = offset + 16;
            if content_offset + entry_header.content_length as usize > buf.len() {
                break;
            }

            let fn_attr = FileNameAttribute::read_from_prefix(&buf[content_offset..])
                .map_err(|_| {
                    FsResolverError::Invalid("Failed to read FileName attribute in index")
                })?
                .0;

            // Read name
            let name_len = fn_attr.filename_length as usize;
            let name_offset = content_offset + core::mem::size_of::<FileNameAttribute>();
            let name_end = name_offset + name_len * 2; // UTF-16

            if name_end <= buf.len() {
                let name_bytes = &buf[name_offset..name_end];
                let name_u16: Vec<u16> = name_bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();

                if let Ok(name) = String::from_utf16(&name_u16)
                    && name != "."
                {
                    let is_dir =
                        (fn_attr.file_attributes & NtfsFileAttributes::DIRECTORY.bits()) != 0;
                    let attr = if is_dir {
                        FileAttributes::new_dir()
                    } else {
                        FileAttributes::new_file()
                    };

                    let (mft_num, _) =
                        crate::utils::parse_mft_reference(entry_header.mft_reference);
                    entries.push((name, mft_num, attr));
                }
            }

            offset += entry_header.entry_length as usize;
        }
        Ok(())
    }

    pub fn resolve_path_internal(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        // Start at root
        let mut current_mft = MFT_RECORD_ROOT;

        // Trim leading slash
        let clean_path = path.trim_start_matches('/');
        if clean_path.empty_or_root() {
            return Ok((true, current_mft as u32, 0));
        }

        for component in clean_path.split('/') {
            if component.is_empty() {
                continue;
            }

            // Read directory entries of current
            let entries = self.read_directory_entries(current_mft)?;

            let upcase = UpcaseHandle::from_flavor(&self.meta.upcase_flavor);
            let mut found = false;
            for (name, mft_num, _) in entries {
                let name_u16: Vec<u16> = name.encode_utf16().collect();
                if crate::utils::eq_names_upcase_str(&name_u16, component, &upcase) {
                    current_mft = mft_num;
                    found = true;
                    break;
                }
            }

            if !found {
                return Ok((false, 0, 0));
            }
        }

        Ok((true, current_mft as u32, 0))
    }
}

trait EmptyOrRoot {
    fn empty_or_root(&self) -> bool;
}

impl EmptyOrRoot for &str {
    fn empty_or_root(&self) -> bool {
        self.is_empty() || *self == "/"
    }
}

impl<'a, 'b, IO: RimIO + ?Sized> FsTreeResolver<'b> for NtfsResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        crate::ensure!(found, FsResolverError::NotFound);

        let entries = self.read_directory_entries(mft_num as u64)?;
        Ok(entries.into_iter().map(|(name, _, _)| name).collect())
    }

    fn open_file(
        &mut self,
        path: &str,
    ) -> FsResolverResult<alloc::boxed::Box<dyn rimio::RimRead + 'b>> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        crate::ensure!(found, FsResolverError::NotFound);

        let record = self.read_mft_record(mft_num as u64)?;
        let header = MftRecordHeader::read_from_prefix(&record).unwrap().0;
        crate::ensure!(!header.is_dir(), FsResolverError::Invalid("not a file"));

        let data = self.read_file(path)?;
        Ok(alloc::boxed::Box::new(rimio::VecRimIO::new(data)))
    }

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        crate::ensure!(found, FsResolverError::NotFound);

        let record = self.read_mft_record(mft_num as u64)?;
        let header = MftRecordHeader::read_from_prefix(&record).unwrap().0;
        crate::ensure!(!header.is_dir(), FsResolverError::Invalid("not a file"));

        if let Some(attr) = self.find_attribute(&record, ATTR_DATA)? {
            if attr.is_resident() {
                let content = self.get_resident_attribute_content(attr)?;
                Ok(content.to_vec())
            } else {
                self.get_non_resident_attribute_content(attr)
            }
        } else {
            Ok(Vec::new()) // Empty file
        }
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        if !found {
            return Err(FsResolverError::NotFound);
        }

        let record = self.read_mft_record(mft_num as u64)?;
        let view = MftRecordView::new(&record)
            .map_err(|_| FsResolverError::Invalid("Failed to parse MFT record"))?;

        let is_dir = view.is_dir();
        let mut attr = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };

        if let Ok(Some(std_info_attr)) = view.find(AttributeType::StandardInformation.code())
            && let Ok(AttrView::Resident { value, .. }) = std_info_attr.as_view()
            && let Ok((std_info, _)) = StandardInformation::read_from_prefix(value)
        {
            let ntfs_attr = NtfsFileAttributes::from_bits_truncate(std_info.file_attributes);
            attr.read_only = ntfs_attr.contains(NtfsFileAttributes::READ_ONLY);
            attr.hidden = ntfs_attr.contains(NtfsFileAttributes::HIDDEN);
            attr.system = ntfs_attr.contains(NtfsFileAttributes::SYSTEM);
            attr.archive = ntfs_attr.contains(NtfsFileAttributes::ARCHIVE);

            attr.created = crate::utils::ntfs_time_to_offset_date_time(std_info.creation_time);
            attr.modified = crate::utils::ntfs_time_to_offset_date_time(std_info.modification_time);
            attr.accessed = crate::utils::ntfs_time_to_offset_date_time(std_info.access_time);
        }

        Ok(attr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{MftRecordBuilder, NtfsAttribute};
    use crate::types::MftRecordHeader;
    use rimfs_core::injector::FsTreeInjector;
    use rimio::prelude::MemRimIO;

    #[test]
    fn test_ntfs_named_data_stream_resolution() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);

        let offset = meta.lcn_to_offset(meta.mft_lcn) + (16 * meta.mft_record_size as u64);
        {
            let mut builder = MftRecordBuilder::new(&mut io, &meta, offset);
            let header = MftRecordHeader::new(16, crate::flags::MftRecordFlags::IN_USE, 1);
            builder.write_header(header).unwrap();
            builder
                .write_attribute(&NtfsAttribute::data_resident(
                    b"DEFAULT_STREAM_DATA".to_vec(),
                ))
                .unwrap();
            builder
                .write_attribute(&NtfsAttribute::data_resident_named(
                    "custom_stream",
                    b"ALTERNATE_STREAM_DATA".to_vec(),
                ))
                .unwrap();
            builder.finalize().unwrap();
        }

        let mut resolver = NtfsResolver::new(&mut io, &meta);

        // Read unnamed default stream
        let default_data = resolver.read_file_stream(16, None).unwrap();
        assert_eq!(default_data, b"DEFAULT_STREAM_DATA");

        // Read named stream
        let named_data = resolver
            .read_file_stream(16, Some("custom_stream"))
            .unwrap();
        assert_eq!(named_data, b"ALTERNATE_STREAM_DATA");

        // Non-existent stream should return NotFound
        let res = resolver.read_file_stream(16, Some("non_existent"));
        assert!(res.is_err());
    }

    #[test]
    fn test_ntfs_read_attributes_full() {
        let meta = NtfsMeta::new(10 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 10 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);

        crate::formatter::NtfsFormatter::new(&mut io, &meta)
            .format(true)
            .unwrap();

        let mut injector = crate::injector::NtfsInjector::new(&mut io, &meta).unwrap();
        let mut custom_attr = FileAttributes::new_file();
        custom_attr.read_only = true;
        custom_attr.hidden = true;
        custom_attr.archive = true;

        let mut tree = crate::core::resolver::FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![crate::core::resolver::FsNode::new_file_from_source(
                "secret.txt",
                alloc::boxed::Box::new(rimio::prelude::VecRimIO::new(b"TopSecret".to_vec())),
                custom_attr,
            )],
        };

        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut resolver = NtfsResolver::new(&mut io, &meta);
        let read_attr = resolver.read_attributes("/secret.txt").unwrap();

        assert!(read_attr.read_only);
        assert!(read_attr.hidden);
        assert!(read_attr.archive);
        assert!(!read_attr.is_dir());
        assert!(read_attr.created.is_some());
        assert!(read_attr.modified.is_some());
    }
}
