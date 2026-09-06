// SPDX-License-Identifier: MIT
//! NTFS file/directory injector
//!
//! Responsible for adding files and directories to an existing NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{RimIO, RimRead};

use crate::allocator::{NtfsAllocator, NtfsHandle};
use crate::attr::NtfsFileAttributesExt;
use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::constant::SECURITY_ID_EVERYONE;
use crate::core::allocator::FsAllocator;
use crate::core::injector::{FsContext, FsTreeInjector};
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

struct DirContext {
    name: alloc::string::String,
    buf: Vec<NtfsIndexEntry>,
    timestamp: u64,
}

/// NTFS node injector
///
/// Handles insertion of files and directories into an NTFS volume.
pub struct NtfsInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: NtfsAllocator<'a>,
    meta: &'a NtfsMeta,
    // Stack holds (Handle, Directory Context including name and accumulated entries)
    stack: Vec<FsContext<NtfsHandle, DirContext>>,
    mft_allocator: mft::MftAllocator<'a>,
    root_existing_runs: Option<rimio::run::RunList>,
}

impl<'a, IO: RimIO + ?Sized> NtfsInjector<'a, IO> {
    /// Create a new NTFS injector
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> FsInjectorResult<Self> {
        let mft_allocator = mft::MftAllocator::new(meta, crate::constant::MFT_RECORD_USNJRNL + 1);
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

    /// Read existing index entries from the root directory's $INDEX_ROOT attribute.
    fn read_existing_root_entries(&mut self) -> FsInjectorResult<Vec<NtfsIndexEntry>> {
        use crate::constant::{ATTR_INDEX_ROOT, MFT_RECORD_ROOT};
        use crate::view::attr_view::AttrView;
        use crate::view::mft_view::MftRecordView;

        // Read root MFT record (record 5)
        let record =
            mft::read_record(self.io, self.meta, MFT_RECORD_ROOT).map_err(FsInjectorError::IO)?;

        // Find $INDEX_ROOT attribute
        let view = MftRecordView::new(&record)
            .map_err(|_| FsInjectorError::Invalid("Failed to read root MFT record"))?;

        let attr_ref = view
            .find(ATTR_INDEX_ROOT)
            .map_err(|_| FsInjectorError::Invalid("Failed to find $INDEX_ROOT attribute"))?
            .ok_or(FsInjectorError::Invalid(
                "Root directory has no $INDEX_ROOT",
            ))?;

        let attr_view = attr_ref
            .as_view()
            .map_err(|_| FsInjectorError::Invalid("Malformed $INDEX_ROOT attribute"))?;

        let content = match attr_view {
            AttrView::Resident { value, .. } => value,
            AttrView::NonResident { .. } => {
                return Err(FsInjectorError::Invalid(
                    "$INDEX_ROOT is unexpectedly non-resident",
                ));
            }
        };

        // Parse Index Root structure:
        // - IndexRootHeader: 16 bytes
        // - IndexNodeHeader: 16 bytes (entries_offset, index_length, etc.)
        // - Index entries...

        if content.len() < 32 {
            return Ok(Vec::new()); // Too small, no entries
        }

        let node_header_offset = 16; // After IndexRootHeader
        let node_header = &content[node_header_offset..];

        if node_header.len() < 16 {
            return Ok(Vec::new());
        }

        // Read IndexNodeHeader fields
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

        // Calculate absolute offsets within the content
        let entries_start = node_header_offset + entries_offset;
        let entries_end = node_header_offset + index_length;

        if entries_start >= content.len() || entries_end > content.len() {
            return Ok(Vec::new());
        }

        // Extract entries, but EXCLUDE the END marker entry
        let entries_data = &content[entries_start..entries_end];
        let mut result = Vec::new();
        let mut offset = 0;

        while offset < entries_data.len() {
            if offset + 16 > entries_data.len() {
                break;
            }

            let entry_header = &entries_data[offset..];
            let entry_length = u16::from_le_bytes([entry_header[8], entry_header[9]]) as usize;
            let flags = u16::from_le_bytes([entry_header[12], entry_header[13]]);

            // Stop at END marker (flag 0x02)
            if (flags & 0x02) != 0 {
                break;
            }

            if entry_length < 16 || offset + entry_length > entries_data.len() {
                break;
            }

            // Copy this entry to result
            // Parse entry into object
            let raw_entry = entries_data[offset..offset + entry_length].to_vec();
            let mut entry = NtfsIndexEntry::from_raw(raw_entry);
            entry.name = entry.name_from_raw();
            result.push(entry);

            offset += entry_length;
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

    /// Flush the MFT record allocation bitmap ($MFT::$BITMAP) to disk.
    pub fn flush_mft_bitmap(&mut self) -> FsInjectorResult<()> {
        let rec0 = mft::read_record(self.io, self.meta, 0).map_err(FsInjectorError::IO)?;
        let view = crate::view::mft_view::MftRecordView::new(&rec0)
            .map_err(|_| FsInjectorError::Invalid("Failed to parse Record 0"))?;
        let attr = view
            .find(crate::constant::ATTR_BITMAP)
            .map_err(|_| FsInjectorError::Invalid("Failed to find $BITMAP in Record 0"))?
            .ok_or(FsInjectorError::Invalid("Record 0 has no $BITMAP"))?;

        let attr_view = attr
            .as_view()
            .map_err(|_| FsInjectorError::Invalid("Malformed $BITMAP in Record 0"))?;

        let runlist = match attr_view {
            crate::view::attr_view::AttrView::NonResident { runlist, .. } => runlist,
            _ => {
                return Err(FsInjectorError::Invalid(
                    "$BITMAP in Record 0 is unexpectedly resident",
                ));
            }
        };

        let mut first_lcn = None;
        for run in runlist.iter() {
            if let Some(lcn) = run.lcn {
                first_lcn = Some(lcn);
                break;
            }
        }
        let lcn = first_lcn.ok_or(FsInjectorError::Invalid("$BITMAP has no valid cluster run"))?;
        let offset = self.meta.lcn_to_offset(lcn);

        let cluster_bytes = self.meta.bytes_per_cluster as usize;
        let mut buf = alloc::vec![0u8; cluster_bytes];
        self.io
            .read_at(offset, &mut buf)
            .map_err(FsInjectorError::IO)?;

        let used_records = self.mft_allocator.used_units() as u64;
        for rec in (crate::constant::MFT_RECORD_USNJRNL + 1)..used_records {
            let byte_idx = (rec / 8) as usize;
            if byte_idx < buf.len() {
                buf[byte_idx] |= 1 << (rec % 8);
            }
        }

        self.io
            .write_at(offset, &buf)
            .map_err(FsInjectorError::IO)?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<NtfsHandle> for NtfsInjector<'a, IO> {
    fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
        // Root MFT record is 5
        let handle = NtfsHandle::new(5);

        // Read existing allocation runs from Record 5 if any
        self.root_existing_runs = self.read_existing_allocation_runs(5);

        // Populate with the canonical 12 system entries for Root ($MFT, $LogFile, etc.)
        let mut entries = crate::features::root::NtfsRootDirFeature::build_root_entries(
            self.meta,
            crate::utils::current_ntfs_time(),
        );

        // Also check if any existing user entries were present in the root directory
        let existing_user_entries = self.read_existing_root_entries().unwrap_or_default();
        for entry in existing_user_entries {
            if !entries.iter().any(|e| e.name == entry.name) {
                entries.push(entry);
            }
        }

        let timestamp = crate::utils::current_ntfs_time();
        self.stack.push(FsContext::new(
            handle,
            DirContext {
                name: ".".into(),
                buf: entries,
                timestamp,
            },
        ));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let timestamp = crate::utils::current_ntfs_time();
        let mft_num = self.allocate_mft_record()?;
        let mft_ref = system_file_mft_reference(mft_num);

        // Add to parent index immediately
        if let Some(parent_ctx) = self.stack.last_mut() {
            let parent_mft = parent_ctx.handle.start_lcn;
            let parent_ref = system_file_mft_reference(parent_mft);
            let ns = determine_file_name_namespace(name);

            let name_utf16: Vec<u16> = name.encode_utf16().collect();
            let entry = NtfsIndexEntry::new(
                mft_ref,
                parent_ref,
                name_utf16,
                attr.as_ntfs_attr() | NtfsFileAttributes::DIRECTORY | NtfsFileAttributes::I30_INDEX,
                IndexEntryFlags::empty(),
                None,
            )
            .with_timestamps(timestamp)
            .with_namespace(ns)
            .with_sizes(0, 0);
            parent_ctx.buf.buf.push(entry);
        }

        // Push child context
        let handle = NtfsHandle::new(mft_num);
        self.stack.push(FsContext::new(
            handle,
            DirContext {
                name: name.into(),
                buf: Vec::new(),
                timestamp,
            },
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
        let mft_ref = system_file_mft_reference(mft_num);

        let parent_mft = self
            .stack
            .last()
            .ok_or(FsInjectorError::StackUnderflow)?
            .handle
            .start_lcn;
        let parent_ref = system_file_mft_reference(parent_mft);

        // Use Builder!
        let mut record = NtfsMftRecord::new(mft_num as u32, false, true);
        let ntfs_attr = attr.as_ntfs_attr();

        record.add_attribute(NtfsAttribute::standard_info_custom(
            ntfs_attr,
            SECURITY_ID_EVERYONE,
            timestamp,
        ));

        let mut allocated_size = 0u64;
        let data_attr;

        if size > 0 {
            let resident_max = (self.meta.mft_record_size as usize).saturating_sub(400);
            if (size as usize) < resident_max {
                allocated_size = size;
                let mut data = vec![0u8; size as usize];
                source.read_at(0, &mut data).map_err(FsInjectorError::IO)?;
                data_attr = NtfsAttribute::data_resident(data);
            } else {
                let clusters = size.div_ceil(self.meta.bytes_per_cluster as u64);
                allocated_size = clusters * self.meta.bytes_per_cluster as u64;
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
                data_attr = NtfsAttribute::non_resident(
                    AttributeType::Data,
                    "",
                    self.meta,
                    &handle.runs,
                    size,
                );
            }
        } else {
            data_attr = NtfsAttribute::data_empty();
        }

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

        let name_utf16: Vec<u16> = name.encode_utf16().collect();
        let entry = NtfsIndexEntry::new(
            mft_ref,
            parent_ref,
            name_utf16,
            ntfs_attr,
            IndexEntryFlags::empty(),
            None,
        )
        .with_timestamps(timestamp)
        .with_namespace(ns)
        .with_sizes(size, allocated_size);
        ctx.buf.buf.push(entry);

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
            let mft_num = ctx.handle.start_lcn;
            let is_root = mft_num == 5;

            let attrs = if is_root {
                NtfsFileAttributes::HIDDEN
                    | NtfsFileAttributes::SYSTEM
                    | NtfsFileAttributes::DIRECTORY
            } else {
                NtfsFileAttributes::DIRECTORY
            };

            let file_name_attrs = if is_root {
                (attrs - NtfsFileAttributes::DIRECTORY) | NtfsFileAttributes::I30_INDEX
            } else {
                attrs | NtfsFileAttributes::I30_INDEX
            };

            let parent_ref = if let Some(p) = self.stack.last() {
                system_file_mft_reference(p.handle.start_lcn)
            } else {
                system_file_mft_reference(5) // Root self-ref (record 5, sequence 5)
            };

            let timestamp = ctx.buf.timestamp;
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

            if is_root {
                record.add_attribute(NtfsAttribute::file_name_custom(
                    parent_ref,
                    &ctx.buf.name,
                    0,
                    file_name_attrs,
                    NtfsFileNameNamespace::Win32AndDos,
                    timestamp,
                ));
            } else {
                let ns = determine_file_name_namespace(&ctx.buf.name);
                record.add_attribute(NtfsAttribute::file_name_custom(
                    parent_ref,
                    &ctx.buf.name,
                    0,
                    file_name_attrs,
                    ns,
                    timestamp,
                ));
            }

            if is_root {
                record.add_attribute(NtfsAttribute::security_descriptor(
                    SECURITY_DESCRIPTOR_ROOT.to_vec(),
                ));
            }

            // Build Index ($I30) using unified DirectoryIndexResult
            let index_res = IndexTreeBuilder::build_directory_index(self.meta, ctx.buf.buf)
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

        self.flush_mft_bitmap()?;
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
