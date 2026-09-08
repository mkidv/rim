// SPDX-License-Identifier: MIT
//! NTFS file/directory injector
//!
//! Responsible for adding files and directories to an existing NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::{RimIO, RimRead};

use crate::allocator::{NtfsAllocator, NtfsHandle};
use crate::attr::NtfsFileAttributesExt;
use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::constant::SECURITY_ID_EVERYONE;
use crate::core::allocator::FsAllocator;
use crate::core::injector::FsTreeInjector;
use crate::core::resolver::attr::FileAttributes;
use crate::core::{FsInjectorError, FsInjectorResult};
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::security::SECURITY_DESCRIPTOR_ROOT;
use crate::types::{
    IndexEntryFlags, IndexTreeBuilder, NtfsAttribute, NtfsFileAttributes, NtfsIndexEntry,
    NtfsMftRecord,
};
use crate::utils::*;

/// NTFS directory context tracking open directory state and child entries
struct NtfsContext {
    handle: NtfsHandle,
    name: alloc::string::String,
    entries: Vec<NtfsIndexEntry>,
    timestamp: u64,
    file_attrs: NtfsFileAttributes,
}

impl NtfsContext {
    fn new(
        handle: NtfsHandle,
        name: alloc::string::String,
        entries: Vec<NtfsIndexEntry>,
        timestamp: u64,
        file_attrs: NtfsFileAttributes,
    ) -> Self {
        Self {
            handle,
            name,
            entries,
            timestamp,
            file_attrs,
        }
    }

    #[inline]
    fn mft_num(&self) -> u64 {
        self.handle.start_lcn
    }

    /// Add a child directory entry (or dual entries with DOS 8.3 alias if name exceeds DOS 8.3)
    fn push_dir_entry(
        &mut self,
        child_mft: u64,
        name: &str,
        attrs: NtfsFileAttributes,
        timestamp: u64,
    ) {
        let child_ref = system_file_mft_reference(child_mft);
        let parent_ref = system_file_mft_reference(self.mft_num());

        if is_valid_dos_8_3(name) {
            let name_utf16: Vec<u16> = name.encode_utf16().collect();
            let entry = NtfsIndexEntry::new(
                child_ref,
                parent_ref,
                name_utf16,
                attrs,
                IndexEntryFlags::empty(),
                None,
            )
            .with_timestamps(timestamp)
            .with_namespace(NtfsFileNameNamespace::Win32AndDos)
            .with_sizes(0, 0);
            self.entries.push(entry);
        } else {
            let dos_name = generate_dos_8_3_name(name);
            let dos_utf16: Vec<u16> = dos_name.encode_utf16().collect();
            let dos_entry = NtfsIndexEntry::new(
                child_ref,
                parent_ref,
                dos_utf16,
                attrs,
                IndexEntryFlags::empty(),
                None,
            )
            .with_timestamps(timestamp)
            .with_namespace(NtfsFileNameNamespace::Dos)
            .with_sizes(0, 0);
            self.entries.push(dos_entry);

            let win32_utf16: Vec<u16> = name.encode_utf16().collect();
            let win32_entry = NtfsIndexEntry::new(
                child_ref,
                parent_ref,
                win32_utf16,
                attrs,
                IndexEntryFlags::empty(),
                None,
            )
            .with_timestamps(timestamp)
            .with_namespace(NtfsFileNameNamespace::Win32)
            .with_sizes(0, 0);
            self.entries.push(win32_entry);
        }
    }

    /// Add a child file entry
    fn push_file_entry(
        &mut self,
        child_mft: u64,
        name: &str,
        attrs: NtfsFileAttributes,
        size: u64,
        allocated_size: u64,
        timestamp: u64,
    ) {
        let child_ref = system_file_mft_reference(child_mft);
        let parent_ref = system_file_mft_reference(self.mft_num());
        let ns = determine_file_name_namespace(name);
        let name_utf16: Vec<u16> = name.encode_utf16().collect();

        let entry = NtfsIndexEntry::new(
            child_ref,
            parent_ref,
            name_utf16,
            attrs,
            IndexEntryFlags::empty(),
            None,
        )
        .with_timestamps(timestamp)
        .with_namespace(ns)
        .with_sizes(size, allocated_size);
        self.entries.push(entry);
    }
}

/// NTFS node injector
///
/// Handles insertion of files and directories into an NTFS volume.
pub struct NtfsInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: NtfsAllocator<'a>,
    meta: &'a NtfsMeta,
    stack: Vec<NtfsContext>,
    mft_allocator: mft::MftAllocator<'a>,
    root_existing_runs: Option<rimio::run::RunList>,
}

impl<'a, IO: RimIO + ?Sized> NtfsInjector<'a, IO> {
    /// Create a new NTFS injector
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> FsInjectorResult<Self> {
        let mft_allocator =
            mft::MftAllocator::from_io(io, meta).map_err(FsInjectorError::Allocator)?;
        let allocator = NtfsAllocator::from_io(io, meta).map_err(FsInjectorError::Allocator)?;
        Ok(Self {
            io,
            allocator,
            mft_allocator,
            meta,
            stack: Vec::new(),
            root_existing_runs: None,
        })
    }

    /// Allocate a new MFT record number
    fn allocate_mft_record(&mut self) -> FsInjectorResult<u64> {
        self.mft_allocator
            .allocate_unit(self.io)
            .map(|h| h.0)
            .map_err(FsInjectorError::Allocator)
    }

    /// Read existing index entries from a directory MFT record (both $INDEX_ROOT and $INDEX_ALLOCATION).
    fn read_existing_directory_entries(
        &mut self,
        record_number: u64,
    ) -> FsInjectorResult<Vec<NtfsIndexEntry>> {
        use crate::constant::{ATTR_INDEX_ALLOCATION, ATTR_INDEX_ROOT};
        use crate::view::attr_view::AttrView;
        use crate::view::mft_view::MftRecordView;

        let record =
            mft::read_record(self.io, self.meta, record_number).map_err(FsInjectorError::IO)?;
        let view = MftRecordView::new(&record)
            .map_err(|_| FsInjectorError::Invalid("Failed to read directory MFT record"))?;

        let mut result = Vec::new();

        // 1. Read resident entries from $INDEX_ROOT
        if let Ok(Some(attr_ref)) = view.find_named(ATTR_INDEX_ROOT, Some("$I30"))
            && let Ok(AttrView::Resident { value, .. }) = attr_ref.as_view()
            && value.len() >= 32
        {
            let node_header = &value[16..];
            let entries_offset = u32::from_le_bytes([
                node_header[0],
                node_header[1],
                node_header[2],
                node_header[3],
            ]) as usize;
            let index_length = u32::from_le_bytes([
                node_header[4],
                node_header[5],
                node_header[6],
                node_header[7],
            ]) as usize;

            let entries_start = 16 + entries_offset;
            let entries_end = 16 + index_length;
            if entries_start < value.len() && entries_end <= value.len() {
                let entries_data = &value[entries_start..entries_end];
                let mut offset = 0;
                while offset + 16 <= entries_data.len() {
                    let entry_header = &entries_data[offset..];
                    let entry_length =
                        u16::from_le_bytes([entry_header[8], entry_header[9]]) as usize;
                    let flags = u16::from_le_bytes([entry_header[12], entry_header[13]]);
                    if (flags & 0x02) != 0 {
                        break;
                    }
                    if entry_length < 16 || offset + entry_length > entries_data.len() {
                        break;
                    }
                    let raw_entry = &entries_data[offset..offset + entry_length];
                    if let Some(entry) = NtfsIndexEntry::from_raw(raw_entry)
                        && !result.iter().any(|e: &NtfsIndexEntry| e.name == entry.name)
                    {
                        result.push(entry);
                    }
                    offset += entry_length;
                }
            }
        }

        // 2. Read non-resident entries from $INDEX_ALLOCATION
        if let Ok(Some(attr_ref)) = view.find_named(ATTR_INDEX_ALLOCATION, Some("$I30"))
            && let Ok(AttrView::NonResident { runlist, .. }) = attr_ref.as_view()
        {
            let block_size = self.meta.index_record_size as usize;
            let cluster_bytes = self.meta.bytes_per_cluster as usize;

            for run in runlist.iter() {
                if let Some(lcn) = run.lcn {
                    let offset = self.meta.lcn_to_offset(lcn);
                    let run_bytes = run.len as usize * cluster_bytes;
                    let mut buf = alloc::vec![0u8; run_bytes];
                    self.io
                        .read_at(offset, &mut buf)
                        .map_err(FsInjectorError::IO)?;

                    for chunk in buf.chunks_exact_mut(block_size) {
                        if chunk.len() < 4 || &chunk[0..4] != b"INDX" {
                            continue;
                        }
                        if !crate::utils::decode_usa_fixup(
                            chunk,
                            self.meta.bytes_per_sector as usize,
                        ) {
                            continue;
                        }

                        if chunk.len() < 40 {
                            continue;
                        }

                        let node_header = &chunk[24..];
                        let entries_offset = u32::from_le_bytes([
                            node_header[0],
                            node_header[1],
                            node_header[2],
                            node_header[3],
                        ]) as usize;
                        let index_length = u32::from_le_bytes([
                            node_header[4],
                            node_header[5],
                            node_header[6],
                            node_header[7],
                        ]) as usize;

                        let entries_start = 24 + entries_offset;
                        let entries_end = 24 + index_length;
                        if entries_start < chunk.len() && entries_end <= chunk.len() {
                            let entries_data = &chunk[entries_start..entries_end];
                            let mut offset = 0;
                            while offset + 16 <= entries_data.len() {
                                let entry_header = &entries_data[offset..];
                                let entry_length =
                                    u16::from_le_bytes([entry_header[8], entry_header[9]]) as usize;
                                let flags =
                                    u16::from_le_bytes([entry_header[12], entry_header[13]]);
                                if (flags & 0x02) != 0 {
                                    break;
                                }
                                if entry_length < 16 || offset + entry_length > entries_data.len() {
                                    break;
                                }
                                let raw_entry = &entries_data[offset..offset + entry_length];
                                if let Some(entry) = NtfsIndexEntry::from_raw(raw_entry)
                                    && !result.iter().any(|e: &NtfsIndexEntry| e.name == entry.name)
                                {
                                    result.push(entry);
                                }
                                offset += entry_length;
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Read existing allocation runs from an MFT record's $INDEX_ALLOCATION attribute if present.
    fn read_existing_allocation_runs(&mut self, record_number: u64) -> Option<rimio::run::RunList> {
        use crate::constant::ATTR_INDEX_ALLOCATION;
        use crate::view::attr_view::AttrView;
        use crate::view::mft_view::MftRecordView;

        let record = mft::read_record(self.io, self.meta, record_number).ok()?;
        let view = MftRecordView::new(&record).ok()?;
        let attr_ref = view
            .find_named(ATTR_INDEX_ALLOCATION, Some("$I30"))
            .ok()??;
        let attr_view = attr_ref.as_view().ok()?;

        if let AttrView::NonResident { runlist, .. } = attr_view {
            let mut rl = rimio::run::RunList::new();
            for run in runlist.iter() {
                if let Some(lcn) = run.lcn {
                    rl.push(rimio::run::Run::new(lcn, run.len));
                }
            }
            if !rl.is_empty() {
                return Some(rl);
            }
        }
        None
    }

    /// Build file $DATA attribute (resident or non-resident) and return it along with allocated byte size.
    fn build_file_data_attribute(
        &mut self,
        source: &mut dyn RimRead,
        size: u64,
    ) -> FsInjectorResult<(NtfsAttribute<'static>, u64)> {
        if size == 0 {
            return Ok((NtfsAttribute::data_empty(), 0));
        }

        let resident_max = (self.meta.mft_record_size as usize).saturating_sub(400);
        if (size as usize) < resident_max {
            let mut data = vec![0u8; size as usize];
            source.read_at(0, &mut data).map_err(FsInjectorError::IO)?;
            Ok((NtfsAttribute::data_resident(data), size))
        } else {
            let clusters = size.div_ceil(self.meta.bytes_per_cluster as u64);
            let allocated_size = clusters * self.meta.bytes_per_cluster as u64;
            let handle = self
                .allocator
                .allocate_contiguous(self.io, clusters as usize)?;

            crate::core::utils::stream_copy::write_stream_to_run_list(
                self.io,
                self.meta,
                source,
                &handle.runs,
                size,
            )
            .map_err(FsInjectorError::IO)?;
            let attr =
                NtfsAttribute::non_resident(AttributeType::Data, "", self.meta, &handle.runs, size);
            Ok((attr, allocated_size))
        }
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<NtfsHandle> for NtfsInjector<'a, IO> {
    fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
        // Root MFT record is 5
        let handle = NtfsHandle::new(5);

        // Read existing allocation runs from Record 5 if any
        self.root_existing_runs = self.read_existing_allocation_runs(5);

        // Read existing entries from disk if possible (both resident and non-resident INDX)
        let mut entries = self.read_existing_directory_entries(5).unwrap_or_default();
        if entries.is_empty() {
            entries = crate::features::root::NtfsRootDirFeature::build_root_entries(
                self.meta,
                crate::utils::current_ntfs_time(),
            );
        }

        // If "." entry exists in entries, use its creation time as timestamp
        let dot_name: Vec<u16> = ".".encode_utf16().collect();
        let timestamp = entries
            .iter()
            .find(|e| e.name == dot_name)
            .map(|e| e.creation_time)
            .unwrap_or_else(crate::utils::current_ntfs_time);

        self.stack.push(NtfsContext::new(
            handle,
            ".".into(),
            entries,
            timestamp,
            NtfsFileAttributes::DIRECTORY
                | NtfsFileAttributes::HIDDEN
                | NtfsFileAttributes::SYSTEM
                | NtfsFileAttributes::I30_INDEX,
        ));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let timestamp = crate::utils::current_ntfs_time();
        let mft_num = self.allocate_mft_record()?;
        let dir_file_name_attrs =
            (attr.as_ntfs_attr() - NtfsFileAttributes::DIRECTORY) | NtfsFileAttributes::I30_INDEX;

        // Add to parent index immediately
        if let Some(parent_ctx) = self.stack.last_mut() {
            parent_ctx.push_dir_entry(mft_num, name, dir_file_name_attrs, timestamp);
        }

        // Push child context
        let handle = NtfsHandle::new(mft_num);
        self.stack.push(NtfsContext::new(
            handle,
            name.into(),
            Vec::new(),
            timestamp,
            dir_file_name_attrs,
        ));
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let timestamp = crate::utils::current_ntfs_time();
        let mft_num = self.allocate_mft_record()?;
        let parent_mft = self
            .stack
            .last()
            .ok_or(FsInjectorError::StackUnderflow)?
            .mft_num();
        let parent_ref = system_file_mft_reference(parent_mft);

        let mut record = NtfsMftRecord::new(mft_num as u32, false, true);
        let ntfs_attr = attr.as_ntfs_attr();

        record.add_attribute(NtfsAttribute::standard_info_custom(
            ntfs_attr,
            SECURITY_ID_EVERYONE,
            timestamp,
        ));

        let (data_attr, allocated_size) = self.build_file_data_attribute(source, size)?;
        let ns = determine_file_name_namespace(name);

        // Attr 0x30 ($FILE_NAME) MUST come before Attr 0x80 ($DATA)
        record.add_attribute(NtfsAttribute::file_name_with_sizes_custom(
            parent_ref,
            name,
            allocated_size,
            size,
            ntfs_attr,
            ns,
            timestamp,
        ));

        record.add_attribute(data_attr);

        let raw = record
            .to_raw_buffer(self.meta)
            .map_err(FsInjectorError::IO)?;
        mft::write_record(self.io, self.meta, mft_num, &raw).map_err(FsInjectorError::IO)?;

        // Add to parent index
        let ctx = self
            .stack
            .last_mut()
            .ok_or(FsInjectorError::StackUnderflow)?;
        ctx.push_file_entry(mft_num, name, ntfs_attr, size, allocated_size, timestamp);

        Ok(())
    }

    fn write_symlink(
        &mut self,
        _name: &str,
        _target: &str,
        _attr: &FileAttributes,
    ) -> FsInjectorResult {
        Err(FsInjectorError::Unsupported(
            "NTFS does not support symbolic links yet",
        ))
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(ctx) = self.stack.pop() {
            let mft_num = ctx.mft_num();
            let is_root = mft_num == 5;

            let file_name_attrs = if is_root {
                (NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM)
                    | NtfsFileAttributes::I30_INDEX
            } else {
                ctx.file_attrs
            };

            let attrs = if is_root {
                NtfsFileAttributes::HIDDEN
                    | NtfsFileAttributes::SYSTEM
                    | NtfsFileAttributes::DIRECTORY
            } else {
                ctx.file_attrs - NtfsFileAttributes::I30_INDEX
            };

            let parent_ref = if let Some(p) = self.stack.last() {
                system_file_mft_reference(p.mft_num())
            } else {
                system_file_mft_reference(5) // Root self-ref (record 5, sequence 5)
            };

            let timestamp = ctx.timestamp;
            let mut record = NtfsMftRecord::new(mft_num as u32, true, true);
            if is_root {
                record.add_attribute(NtfsAttribute::standard_info_basic(attrs, timestamp));
            } else {
                record.add_attribute(NtfsAttribute::standard_info_custom(
                    attrs,
                    SECURITY_ID_EVERYONE,
                    timestamp,
                ));
            }

            if is_root || is_valid_dos_8_3(&ctx.name) {
                record.add_attribute(NtfsAttribute::file_name_custom(
                    parent_ref,
                    &ctx.name,
                    0,
                    file_name_attrs,
                    NtfsFileNameNamespace::Win32AndDos,
                    timestamp,
                ));
            } else {
                let dos_name = generate_dos_8_3_name(&ctx.name);
                record.header.link_count = 2;
                record.add_attribute(NtfsAttribute::file_name_custom(
                    parent_ref,
                    &dos_name,
                    0,
                    file_name_attrs,
                    NtfsFileNameNamespace::Dos,
                    timestamp,
                ));
                record.add_attribute(NtfsAttribute::file_name_custom(
                    parent_ref,
                    &ctx.name,
                    0,
                    file_name_attrs,
                    NtfsFileNameNamespace::Win32,
                    timestamp,
                ));
            }

            if is_root {
                record.add_attribute(NtfsAttribute::security_descriptor(
                    SECURITY_DESCRIPTOR_ROOT.to_vec(),
                ));
            }

            // Build Index ($I30) using unified DirectoryIndexResult
            let index_res = IndexTreeBuilder::build_directory_index(self.meta, ctx.entries)
                .map_err(FsInjectorError::IO)?;

            let clusters_per_index = self.meta.clusters_per_index_record_raw();

            match index_res {
                crate::types::DirectoryIndexResult::Resident { entries_buf } => {
                    let old_runs = if is_root {
                        self.root_existing_runs.take()
                    } else {
                        None
                    };
                    if let Some(old) = old_runs {
                        for r in old.iter() {
                            self.allocator
                                .free_range(self.io, r.start, r.length)
                                .map_err(FsInjectorError::Allocator)?;
                        }
                    }
                    record.add_attribute(NtfsAttribute::index_root_i30(
                        entries_buf,
                        clusters_per_index,
                        self.meta.index_record_size,
                        false,
                    ));
                }
                crate::types::DirectoryIndexResult::NonResident { layout } => {
                    let runs = if is_root
                        && self.root_existing_runs.as_ref().map(|r| r.total_units())
                            == Some(layout.total_clusters)
                    {
                        // Reuse existing cluster runs! No new cluster allocated!
                        self.root_existing_runs.take().unwrap()
                    } else {
                        let old_runs = if is_root {
                            self.root_existing_runs.take()
                        } else {
                            None
                        };
                        if let Some(old) = old_runs {
                            for r in old.iter() {
                                self.allocator
                                    .free_range(self.io, r.start, r.length)
                                    .map_err(FsInjectorError::Allocator)?;
                            }
                        }
                        let handle = self
                            .allocator
                            .allocate_contiguous(self.io, layout.total_clusters as usize)
                            .map_err(FsInjectorError::Allocator)?;
                        handle.runs
                    };

                    layout
                        .write_allocation_blocks(self.io, self.meta, &runs)
                        .map_err(FsInjectorError::IO)?;

                    record.add_attribute(NtfsAttribute::index_root_i30(
                        layout.root_entries,
                        clusters_per_index,
                        self.meta.index_record_size,
                        true,
                    ));

                    record.add_attribute(NtfsAttribute::non_resident(
                        AttributeType::IndexAllocation,
                        "$I30",
                        self.meta,
                        &runs,
                        layout.total_clusters * self.meta.bytes_per_cluster as u64,
                    ));
                    record.add_attribute(NtfsAttribute::bitmap_named("$I30", layout.bitmap));
                }
            }

            let raw = record
                .to_raw_buffer(self.meta)
                .map_err(FsInjectorError::IO)?;
            mft::write_record(self.io, self.meta, mft_num, &raw).map_err(FsInjectorError::IO)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        while !self.stack.is_empty() {
            self.flush_current()?;
        }

        self.mft_allocator
            .flush(self.io)
            .map_err(FsInjectorError::Allocator)?;
        self.allocator
            .flush(self.io)
            .map_err(FsInjectorError::Allocator)?;
        self.io.flush().map_err(FsInjectorError::IO)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::NtfsChecker;
    use crate::resolver::NtfsResolver;
    use rimfs_core::checker::FsChecker;
    use rimfs_core::testing::{ExpectedFile, assert_files, assert_no_errors, nested_files_tree};
    use rimio::prelude::MemRimIO;

    #[test]
    fn test_ntfs_injector_flow() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, Some("TEST")).unwrap();
        let mut buffer = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        crate::formatter::NtfsFormatter::new(&mut io, &meta)
            .format(true)
            .unwrap();

        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
        let mut tree = nested_files_tree();

        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut checker = NtfsChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert_no_errors(&report);

        let mut resolver = NtfsResolver::new(&mut io, &meta);
        assert_files(
            &mut resolver,
            &[
                ExpectedFile {
                    path: "/subdir/hello.txt",
                    bytes: b"Hello World!",
                },
                ExpectedFile {
                    path: "/readme.md",
                    bytes: b"Test Readme",
                },
            ],
        );
    }
}
