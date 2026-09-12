// SPDX-License-Identifier: MIT
//! NTFS filesystem resolver (parser)
//!
//! Responsible for reading and traversing an existing NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{collections::BTreeMap, string::String, vec, vec::Vec};
#[cfg(feature = "std")]
use std::collections::BTreeMap;

use rimio::prelude::*;
use zerocopy::FromBytes;

use crate::attr::NtfsFileAttributes;
use crate::constant::*;
use crate::core::errors::{FsResolverError, FsResolverResult};
use crate::core::resolver::FsTreeResolver;
use crate::core::resolver::attr::FileAttributes;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::*;
use crate::upcase::UpcaseHandle;
use crate::view::attr_view::{AttrRef, AttrView};
use crate::view::mft_view::MftRecordView;

pub struct NtfsResolver<'a, IO: RimRead + ?Sized> {
    pub io: &'a mut IO,
    pub meta: &'a NtfsMeta,
    upcase: UpcaseHandle,
    dir_cache: BTreeMap<u64, Vec<(String, u64, FileAttributes)>>,
    mft_cache: BTreeMap<u64, Vec<u8>>,
    mft_runs: Vec<crate::view::runlist::NtfsRun>,
}

impl<'a, IO: RimRead + ?Sized> NtfsResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> Self {
        let upcase = UpcaseHandle::from_flavor(&meta.upcase_flavor);
        Self {
            io,
            meta,
            upcase,
            dir_cache: BTreeMap::new(),
            mft_cache: BTreeMap::new(),
            mft_runs: Vec::new(),
        }
    }

    /// Read an MFT record by its record number
    pub fn read_mft_record(&mut self, record_number: u64) -> FsResolverResult<Vec<u8>> {
        if let Some(cached) = self.mft_cache.get(&record_number) {
            return Ok(cached.clone());
        }

        if self.mft_runs.is_empty() {
            let rec0 = mft::read_record_direct(self.io, self.meta, 0)?;
            self.mft_runs = mft::extract_mft_runs(&rec0)?;
            if self.mft_runs.is_empty() {
                return Err(FsResolverError::Invalid("Missing MFT runs"));
            }
            self.mft_cache.insert(0, rec0.clone());
            if record_number == 0 {
                return Ok(rec0);
            }
        }
        let record = mft::read_record_from_runs(self.io, self.meta, record_number, &self.mft_runs)?;

        if self.mft_cache.len() >= 128 {
            self.mft_cache.retain(|&k, _| k == 0);
        }
        self.mft_cache.insert(record_number, record.clone());
        Ok(record)
    }

    /// Find an attribute in an MFT record buffer
    pub fn find_attribute<'b>(
        &self,
        record: &'b [u8],
        attr_type: NtfsAttributeType,
    ) -> FsResolverResult<Option<AttrRef<'b>>> {
        self.find_attribute_named(record, attr_type, None)
    }

    /// Find an attribute in an MFT record buffer with optional stream name
    pub fn find_attribute_named<'b>(
        &self,
        record: &'b [u8],
        attr_type: NtfsAttributeType,
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
            .find_attribute_named(&record, NtfsAttributeType::Data, stream_name)?
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
        if (attr.header.flags
            & (AttributeFlags::COMPRESSED.bits() | AttributeFlags::ENCRYPTED.bits()))
            != 0
        {
            return Err(FsResolverError::Unsupported);
        }

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
        if (attr.header.flags
            & (AttributeFlags::COMPRESSED.bits() | AttributeFlags::ENCRYPTED.bits()))
            != 0
        {
            return Err(FsResolverError::Unsupported);
        }

        let view = attr
            .as_view()
            .map_err(|_| FsResolverError::Invalid("Malformed attribute"))?;

        let (runlist, data_size, initialized_size) = match view {
            AttrView::NonResident {
                runlist,
                data_size,
                initialized_size,
                ..
            } => (runlist, data_size, initialized_size),
            AttrView::Resident { .. } => {
                return Err(FsResolverError::Invalid("Attribute is resident"));
            }
        };

        let data_len = usize::try_from(data_size)
            .map_err(|_| FsResolverError::Invalid("Non-resident attribute is too large"))?;
        let mut content = Vec::with_capacity(data_len);
        let cluster_size = self.meta.bytes_per_cluster as u64;
        let total_len = self.io.total_size().ok();

        for run in runlist.iter() {
            if content.len() as u64 >= data_size {
                break;
            }

            let run_bytes = run
                .len
                .checked_mul(cluster_size)
                .ok_or(FsResolverError::Invalid("Run length overflow"))?;
            let remaining = data_size - content.len() as u64;
            let read_bytes = run_bytes.min(remaining);
            let current_pos = content.len() as u64;

            match run.lcn {
                Some(lcn) => {
                    if current_pos >= initialized_size {
                        let new_len = content.len() + read_bytes as usize;
                        content.resize(new_len, 0);
                    } else {
                        let valid_in_run = (initialized_size - current_pos).min(read_bytes);
                        let valid_len = usize::try_from(valid_in_run)
                            .map_err(|_| FsResolverError::Invalid("Run is too large"))?;

                        let offset = self.meta.lcn_to_offset(lcn);
                        let end = offset
                            .checked_add(valid_in_run)
                            .ok_or(FsResolverError::Invalid("Run offset overflow"))?;
                        if total_len.is_some_and(|total| end > total) {
                            return Err(FsResolverError::Invalid("Run exceeds volume bounds"));
                        }

                        let mut buf = vec![0u8; valid_len];
                        self.io
                            .read_at(offset, &mut buf)
                            .map_err(FsResolverError::IO)?;

                        content.extend_from_slice(&buf);

                        if read_bytes > valid_in_run {
                            let zero_len = (read_bytes - valid_in_run) as usize;
                            let new_len = content.len() + zero_len;
                            content.resize(new_len, 0);
                        }
                    }
                }
                None => {
                    let new_len = content
                        .len()
                        .checked_add(read_bytes as usize)
                        .ok_or(FsResolverError::Invalid("Sparse run length overflow"))?;
                    content.resize(new_len, 0);
                }
            }
        }

        if content.len() != data_len {
            return Err(FsResolverError::Invalid(
                "Incomplete non-resident allocation",
            ));
        }
        Ok(content)
    }

    /// Read the index root of a directory
    pub(crate) fn _read_index_root(&self, record: &[u8]) -> FsResolverResult<Vec<u8>> {
        let attr = self
            .find_attribute(record, NtfsAttributeType::IndexRoot)?
            .ok_or(FsResolverError::Invalid("Directory has no $INDEX_ROOT"))?;

        let content = self.get_resident_attribute_content(attr)?;
        Ok(content.to_vec())
    }

    /// Read entries from a directory MFT record
    ///
    /// Supports resident indexes in $INDEX_ROOT and non-resident in $INDEX_ALLOCATION
    /// Ensure directory entries for an MFT record are loaded into cache
    pub fn load_directory_entries(&mut self, record_number: u64) -> FsResolverResult<()> {
        if self.dir_cache.contains_key(&record_number) {
            return Ok(());
        }

        let record = self.read_mft_record(record_number)?;

        let header = MftRecordHeader::ref_from_prefix(&record)
            .map_err(|_| FsResolverError::Invalid("Failed to read MFT header"))?
            .0;

        if !header.is_dir() {
            return Err(FsResolverError::Invalid("Not a directory"));
        }

        let mut entries = Vec::new();

        // 1. Read Resident $INDEX_ROOT ($I30 or unnamed)
        let index_root_attr =
            match self.find_attribute_named(&record, NtfsAttributeType::IndexRoot, Some("$I30"))? {
                Some(a) => Some(a),
                None => self.find_attribute(&record, NtfsAttributeType::IndexRoot)?,
            };

        if let Some(attr) = index_root_attr {
            let content = self.get_resident_attribute_content(attr)?;

            let _root_header = IndexRootHeader::ref_from_prefix(content)
                .map_err(|_| FsResolverError::Invalid("Failed to read Index Root Header"))?
                .0;

            let node_header_offset = core::mem::size_of::<IndexRootHeader>();
            if node_header_offset < content.len() {
                self.parse_entries_from_node_header(&content[node_header_offset..], &mut entries)?;
            }
        }

        // 2. Read Non-Resident $INDEX_ALLOCATION ($I30 or unnamed)
        let index_alloc_attr = match self.find_attribute_named(
            &record,
            NtfsAttributeType::IndexAllocation,
            Some("$I30"),
        )? {
            Some(a) => Some(a),
            None => self.find_attribute(&record, NtfsAttributeType::IndexAllocation)?,
        };

        if let Some(attr) = index_alloc_attr {
            let content = if let Ok(res) = self.get_resident_attribute_content(attr) {
                res.to_vec()
            } else {
                self.get_non_resident_attribute_content(attr)?
            };

            // Look for $BITMAP for $I30 to filter active index blocks
            let bitmap_bytes = match self.find_attribute_named(
                &record,
                NtfsAttributeType::Bitmap,
                Some("$I30"),
            )? {
                Some(b_attr) => {
                    if let Ok(res) = self.get_resident_attribute_content(b_attr) {
                        Some(res.to_vec())
                    } else {
                        Some(self.get_non_resident_attribute_content(b_attr)?)
                    }
                }
                None => match self.find_attribute(&record, NtfsAttributeType::Bitmap)? {
                    Some(b_attr) => {
                        if let Ok(res) = self.get_resident_attribute_content(b_attr) {
                            Some(res.to_vec())
                        } else {
                            Some(self.get_non_resident_attribute_content(b_attr)?)
                        }
                    }
                    None => {
                        return Err(FsResolverError::Invalid("Missing index allocation bitmap"));
                    }
                },
            };

            // Iterate over blocks (Index Records)
            let block_size = self.meta.index_record_size as usize;
            for (block_idx, chunk) in content.chunks(block_size).enumerate() {
                if chunk.len() < block_size {
                    return Err(FsResolverError::Invalid("Truncated index block"));
                }

                // If bitmap is present, skip blocks whose bit is 0
                if let Some(ref bm) = bitmap_bytes {
                    let byte_idx = block_idx / 8;
                    let bit_idx = block_idx % 8;
                    if byte_idx >= bm.len() {
                        return Err(FsResolverError::Invalid("Truncated index bitmap"));
                    }
                    if (bm[byte_idx] & (1 << bit_idx)) == 0 {
                        continue;
                    }
                }

                if chunk[0..4] != NTFS_INDX_SIGNATURE {
                    return Err(FsResolverError::Invalid("Invalid active index block"));
                }

                // Decode USA fixup
                let mut block = chunk.to_vec();
                if !crate::utils::decode_usa_fixup(&mut block, self.meta.bytes_per_sector as usize)
                {
                    return Err(FsResolverError::Invalid("Invalid index USA fixup"));
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

        if self.dir_cache.len() >= 128 {
            self.dir_cache.clear();
        }
        self.dir_cache.insert(record_number, entries);
        Ok(())
    }

    /// Read entries from a directory MFT record
    ///
    /// Supports resident indexes in $INDEX_ROOT and non-resident in $INDEX_ALLOCATION
    pub fn read_directory_entries(
        &mut self,
        record_number: u64,
    ) -> FsResolverResult<Vec<(String, u64, FileAttributes)>> {
        self.load_directory_entries(record_number)?;
        Ok(self
            .dir_cache
            .get(&record_number)
            .cloned()
            .unwrap_or_default())
    }

    fn parse_entries_from_node_header(
        &self,
        buf: &[u8],
        entries: &mut Vec<(String, u64, FileAttributes)>,
    ) -> FsResolverResult<()> {
        let node_header = IndexNodeHeader::ref_from_prefix(buf)
            .map_err(|_| FsResolverError::Invalid("Failed to read Index Node Header"))?
            .0;

        let start_offset = node_header.entries_offset.get() as usize;
        let end_offset = node_header.index_length.get() as usize; // Total used size

        // Bounds check
        if start_offset >= buf.len() || end_offset > buf.len() {
            return Ok(());
        }

        let mut offset = start_offset;

        while offset < end_offset {
            if offset + 16 > buf.len() {
                break;
            }

            let entry_header = IndexEntryHeader::ref_from_prefix(&buf[offset..])
                .map_err(|_| FsResolverError::Invalid("Failed to read Index Entry Header"))?
                .0;

            if entry_header.entry_length.get() < 16 {
                break;
            }

            // Last entry marker
            if (entry_header.flags & IndexEntryFlags::LAST_ENTRY.bits()) != 0 {
                break;
            }

            // Extract filename attribute body
            let content_offset = offset + 16;
            if content_offset + entry_header.content_length.get() as usize > buf.len() {
                break;
            }

            let fn_attr = FileNameAttribute::ref_from_prefix(&buf[content_offset..])
                .map_err(|_| {
                    FsResolverError::Invalid("Failed to read FileName attribute in index")
                })?
                .0;

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
                    let is_dir = (fn_attr.file_attributes.get()
                        & (NtfsFileAttributes::DIRECTORY.bits()
                            | NtfsFileAttributes::I30_INDEX.bits()))
                        != 0;
                    let attr = if is_dir {
                        FileAttributes::new_dir()
                    } else {
                        FileAttributes::new_file()
                    };

                    let (mft_num, _) =
                        crate::mft::parse_mft_reference(entry_header.mft_reference.get());
                    entries.push((name, mft_num, attr));
                }
            }

            offset += entry_header.entry_length.get() as usize;
        }
        Ok(())
    }

    pub fn resolve_path_internal(&mut self, path: &str) -> FsResolverResult<(bool, u64, usize)> {
        match crate::core::resolver::walker::walk_path(self, path) {
            Ok(Some(entry)) => Ok((true, entry.mft_num, 0)),
            Ok(None) => Ok((true, MFT_RECORD_ROOT, 0)),
            Err(FsResolverError::NotFound) => Ok((false, 0, 0)),
            Err(e) => Err(e),
        }
    }
}

/// Helper for zero-allocation UpCase comparison between UTF-8 and UTF-16
#[inline]
fn eq_utf8_to_utf16_upcase(name: &str, target_u16: &[u16], upcase: &UpcaseHandle) -> bool {
    let mut name_utf16 = name.encode_utf16();
    for &target_char in target_u16 {
        match name_utf16.next() {
            Some(nc) => {
                if upcase.upper(nc) != upcase.upper(target_char) {
                    return false;
                }
            }
            None => return false,
        }
    }
    name_utf16.next().is_none()
}

use crate::core::resolver::walker::WalkerDataSource;

#[derive(Debug, Clone)]
pub struct NtfsDirEntry {
    pub name: String,
    pub mft_num: u64,
    pub is_dir: bool,
    pub attr: FileAttributes,
}

impl<'a, IO: RimRead + ?Sized> WalkerDataSource for NtfsResolver<'a, IO> {
    type Entry = NtfsDirEntry;
    type NodeId = u64;

    fn root_node(&self) -> Self::NodeId {
        MFT_RECORD_ROOT
    }

    fn find_entry(
        &mut self,
        dir_mft: Self::NodeId,
        name: &str,
    ) -> FsResolverResult<Option<Self::Entry>> {
        self.load_directory_entries(dir_mft)?;

        let target_u16: Vec<u16> = name.encode_utf16().collect();
        if let Some(entries) = self.dir_cache.get(&dir_mft) {
            for (entry_name, mft_num, attr) in entries {
                if eq_utf8_to_utf16_upcase(entry_name, &target_u16, &self.upcase) {
                    return Ok(Some(NtfsDirEntry {
                        name: entry_name.clone(),
                        mft_num: *mft_num,
                        is_dir: attr.is_dir(),
                        attr: attr.clone(),
                    }));
                }
            }
        }
        Ok(None)
    }

    fn is_dir(&self, entry: &Self::Entry) -> bool {
        entry.is_dir
    }

    fn entry_node(&self, entry: &Self::Entry) -> Self::NodeId {
        entry.mft_num
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for NtfsResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        crate::ensure!(found, FsResolverError::NotFound);

        self.load_directory_entries(mft_num)?;
        let entries = self.dir_cache.get(&mft_num).unwrap();
        Ok(entries.iter().map(|(name, _, _)| name.clone()).collect())
    }

    fn open_file<'c>(
        &'c mut self,
        path: &str,
    ) -> FsResolverResult<alloc::boxed::Box<dyn rimio::RimRead + 'c>> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        crate::ensure!(found, FsResolverError::NotFound);

        let record = self.read_mft_record(mft_num)?;
        let header = MftRecordHeader::ref_from_prefix(&record)
            .map_err(|_| FsResolverError::Invalid("Failed to read MFT header"))?
            .0;
        crate::ensure!(!header.is_dir(), FsResolverError::Invalid("Not a file"));

        let Some(attr) = self.find_attribute(&record, NtfsAttributeType::Data)? else {
            return Ok(alloc::boxed::Box::new(rimio::SliceRimIO::new(&[])));
        };

        if attr.header.flags
            & (AttributeFlags::COMPRESSED.bits() | AttributeFlags::ENCRYPTED.bits())
            != 0
        {
            return Err(FsResolverError::Unsupported);
        }
        let view = attr
            .as_view()
            .map_err(|_| FsResolverError::Invalid("Malformed attribute"))?;

        match view {
            AttrView::Resident { value, .. } => {
                Ok(alloc::boxed::Box::new(rimio::VecRimIO::new(value.to_vec())))
            }
            AttrView::NonResident {
                runlist,
                data_size,
                initialized_size,
                ..
            } => {
                if initialized_size > data_size {
                    return Err(FsResolverError::Invalid("Invalid initialized size"));
                }
                let cluster_size = self.meta.bytes_per_cluster as u64;
                let mut extents = Vec::new();
                let mut logical_offset = 0u64;

                for run in runlist.iter() {
                    if logical_offset >= data_size {
                        break;
                    }
                    let run_bytes = run
                        .len
                        .checked_mul(cluster_size)
                        .ok_or(FsResolverError::Invalid("Run length overflow"))?;
                    let extent_len = core::cmp::min(run_bytes, data_size - logical_offset);
                    let source_offset = run.lcn.map(|lcn| self.meta.lcn_to_offset(lcn));
                    let valid = initialized_size
                        .saturating_sub(logical_offset)
                        .min(extent_len);
                    if valid > 0 {
                        if let Some(physical) = source_offset {
                            let end = physical
                                .checked_add(valid)
                                .ok_or(FsResolverError::Invalid("Run overflow"))?;
                            if end > self.io.total_size()? {
                                return Err(FsResolverError::Invalid("Run exceeds volume"));
                            }
                        }
                        extents.push(rimio::extent::IoExtent {
                            logical_offset,
                            source_offset,
                            len: valid,
                        });
                    }
                    if valid < extent_len {
                        extents.push(rimio::extent::IoExtent::hole(
                            logical_offset + valid,
                            extent_len - valid,
                        ));
                    }
                    logical_offset = logical_offset
                        .checked_add(extent_len)
                        .ok_or(FsResolverError::Invalid("Extent offset overflow"))?;
                }

                if logical_offset != data_size {
                    return Err(FsResolverError::Invalid("Incomplete file allocation"));
                }
                Ok(alloc::boxed::Box::new(rimio::extent::ExtentRimRead::new(
                    &mut *self.io,
                    extents,
                    data_size,
                )))
            }
        }
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        let (found, mft_num, _) = self.resolve_path_internal(path)?;
        if !found {
            return Err(FsResolverError::NotFound);
        }

        let record = self.read_mft_record(mft_num)?;
        let view = MftRecordView::new(&record)
            .map_err(|_| FsResolverError::Invalid("Failed to parse MFT record"))?;

        let is_dir = view.is_dir();
        let mut attr = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };

        if let Ok(Some(std_info_attr)) = view.find(NtfsAttributeType::StandardInformation)
            && let Ok(AttrView::Resident { value, .. }) = std_info_attr.as_view()
            && let Ok((std_info, _)) = StandardInformationHeader::ref_from_prefix(value)
        {
            let ntfs_attr = NtfsFileAttributes::from_bits_truncate(std_info.file_attributes.get());
            attr.read_only = ntfs_attr.contains(NtfsFileAttributes::READ_ONLY);
            attr.hidden = ntfs_attr.contains(NtfsFileAttributes::HIDDEN);
            attr.system = ntfs_attr.contains(NtfsFileAttributes::SYSTEM);
            attr.archive = ntfs_attr.contains(NtfsFileAttributes::ARCHIVE);

            attr.created =
                crate::utils::ntfs_time_to_offset_date_time(std_info.creation_time.get());
            attr.modified =
                crate::utils::ntfs_time_to_offset_date_time(std_info.modification_time.get());
            attr.accessed = crate::utils::ntfs_time_to_offset_date_time(std_info.access_time.get());
        }

        Ok(attr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AttributeFlags;
    use crate::types::AttributeHeader;
    use crate::types::MftRecordHeader;
    use crate::types::{MftRecordBuilder, NtfsAttribute};
    use rimfs_core::injector::FsTreeInjector;
    use rimfs_core::testing::file_with_attr;
    use rimio::prelude::MemRimIO;
    use zerocopy::IntoBytes;

    #[test]
    fn test_ntfs_named_data_stream_resolution() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);

        crate::formatter::NtfsFormatter::new(&mut io, &meta)
            .format(true)
            .unwrap();

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

        let default_data = resolver.read_file_stream(16, None).unwrap();
        assert_eq!(default_data, b"DEFAULT_STREAM_DATA");

        let named_data = resolver
            .read_file_stream(16, Some("custom_stream"))
            .unwrap();
        assert_eq!(named_data, b"ALTERNATE_STREAM_DATA");

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
            children: vec![file_with_attr("secret.txt", b"TopSecret", custom_attr)],
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

    #[test]
    fn test_ntfs_compressed_attribute_rejection() {
        let meta = NtfsMeta::new(10 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 10 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);

        crate::formatter::NtfsFormatter::new(&mut io, &meta)
            .format(true)
            .unwrap();

        let resolver = NtfsResolver::new(&mut io, &meta);
        let fake_header = AttributeHeader {
            attr_type: (NtfsAttributeType::Data.code()).into(),
            length: (24).into(),
            non_resident: 0,
            name_length: 0,
            name_offset: (0).into(),
            flags: (AttributeFlags::COMPRESSED.bits()).into(),
            attr_id: (1).into(),
        };
        let mut raw = vec![0u8; 64];
        raw[..core::mem::size_of::<AttributeHeader>()].copy_from_slice(fake_header.as_bytes());

        let attr_ref = crate::view::attr_view::AttrRef {
            raw: &raw,
            header: &fake_header,
        };
        assert!(matches!(
            resolver.get_resident_attribute_content(attr_ref),
            Err(FsResolverError::Unsupported)
        ));
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::core::traits::FsTreeInjector;
    #[test]
    fn public_open_honors_flags_and_initialized_length() {
        let meta = NtfsMeta::new(32 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0; 32 * 1024 * 1024];
        let mut io = rimio::MemRimIO::new(&mut disk);
        crate::NtfsFormatter::new(&mut io, &meta)
            .format(false)
            .unwrap();
        {
            let mut injector = crate::NtfsInjector::new(&mut io, &meta).unwrap();
            injector
                .set_root_context(&FileAttributes::new_dir())
                .unwrap();
            let payload = vec![0x5a; 8192];
            let mut source = rimio::SliceRimIO::new(&payload);
            injector
                .write_file("probe", &mut source, 8192, &FileAttributes::new_file())
                .unwrap();
            injector.flush().unwrap();
        }
        let (_, id, _) = NtfsResolver::new(&mut io, &meta)
            .resolve_path_internal("probe")
            .unwrap();
        let record = crate::mft::read_record(&mut io, &meta, id).unwrap();
        let mut offset = u16::from_le_bytes(record[20..22].try_into().unwrap()) as usize;
        while u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap())
            != NtfsAttributeType::Data.code()
        {
            offset +=
                u32::from_le_bytes(record[offset + 4..offset + 8].try_into().unwrap()) as usize;
        }
        assert_eq!(record[offset + 8], 1);
        for flags in [0u16, 1, 0x4000] {
            let mut changed = record.clone();
            changed[offset + 12..offset + 14].copy_from_slice(&flags.to_le_bytes());
            changed[offset + 56..offset + 64].copy_from_slice(&4u64.to_le_bytes());
            crate::utils::apply_usa_fixup(&mut changed, meta.bytes_per_sector as usize);
            crate::mft::write_record(&mut io, &meta, id, &changed).unwrap();
            let mut resolver = NtfsResolver::new(&mut io, &meta);
            if flags != 0 {
                assert!(matches!(
                    resolver.open_file("probe"),
                    Err(FsResolverError::Unsupported)
                ));
            } else {
                let content = resolver.read_file("probe").unwrap();
                assert_eq!(&content[..4], &[0x5a; 4]);
                assert!(content[4..].iter().all(|b| *b == 0));
            }
        }
    }
}
