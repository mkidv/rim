// SPDX-License-Identifier: MIT
//! NTFS file/directory injector
//!
//! Responsible for adding files and directories to an existing NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{RimIO, RimRead, RimWrite};

use crate::allocator::{NtfsAllocator, NtfsHandle};
use crate::attr::NtfsFileAttributesExt;
use crate::attr::{AttributeType, NtfsFileNameNamespace};
use crate::builder::index_layout::IndexTreeBuilder;
use crate::builder::{NtfsAttribute, NtfsIndexEntry, NtfsMftRecord};
use crate::constant::SECURITY_ID_EVERYONE;
use crate::core::allocator::FsAllocator;
use crate::core::injector::{FsContext, FsTreeInjector};
use crate::core::resolver::FsNode;
use crate::core::resolver::attr::FileAttributes;
use crate::core::{FsInjectorError, FsInjectorResult};
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::{IndexEntryFlags, NtfsFileAttributes, security_descriptor_everyone};
use crate::utils::*;

/// NTFS node injector
///
/// Handles insertion of files and directories into an NTFS volume.
pub struct NtfsInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: NtfsAllocator<'a>,
    meta: &'a NtfsMeta,
    // Stack holds (Handle, Accumulated Children Entries)
    stack: Vec<FsContext<NtfsHandle, Vec<NtfsIndexEntry>>>,
    mft_allocator: mft::MftAllocator<'a>,
}

impl<'a, IO: RimIO + ?Sized> NtfsInjector<'a, IO> {
    /// Create a new NTFS injector
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> FsInjectorResult<Self> {
        let mft_allocator = mft::MftAllocator::new(meta, meta.reserved_mft_records);
        let allocator = NtfsAllocator::new(meta)?;
        Ok(Self {
            io,
            allocator,
            meta,
            stack: vec![],
            mft_allocator,
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
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<NtfsHandle> for NtfsInjector<'a, IO> {
    fn set_root_context(&mut self, _node: &FsNode<'_>) -> FsInjectorResult {
        // Root MFT record is 5
        let handle = NtfsHandle::new(5);

        // Read existing root directory entries from $INDEX_ROOT attribute
        // This preserves system file entries ($MFT, $LogFile, $Secure, etc.)
        let existing_entries = self.read_existing_root_entries().unwrap_or_default();

        self.stack.push(FsContext::new(handle, existing_entries));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let mft_num = self.allocate_mft_record()?;
        let mft_ref = if mft_num <= 11 {
            build_mft_reference(mft_num, if mft_num == 0 { 1 } else { mft_num as u16 })
        } else {
            build_mft_reference(mft_num, 1)
        };

        // Add to parent index immediately
        if let Some(parent_ctx) = self.stack.last_mut() {
            let parent_mft = parent_ctx.handle.start_lcn;
            let parent_ref = build_mft_reference(parent_mft, 1); // Mock sequence

            let name_utf16: Vec<u16> = name.encode_utf16().collect();

            let entry = NtfsIndexEntry::new(
                mft_ref,
                parent_ref,
                name_utf16,
                attr.as_ntfs_attr() | NtfsFileAttributes::DIRECTORY,
                IndexEntryFlags::empty(),
                None,
            );
            parent_ctx.buf.push(entry);
        }

        // Push child context
        let handle = NtfsHandle::new(mft_num);
        self.stack.push(FsContext::new(handle, Vec::new()));
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let mft_num = self.allocate_mft_record()?;
        let mft_ref = if mft_num <= 11 {
            build_mft_reference(mft_num, if mft_num == 0 { 1 } else { mft_num as u16 })
        } else {
            build_mft_reference(mft_num, 1)
        };

        let parent_ref = build_mft_reference(
            self.stack
                .last()
                .ok_or(FsInjectorError::StackUnderflow)?
                .handle
                .start_lcn,
            1,
        );

        // Use Builder!
        let mut record = NtfsMftRecord::new(mft_num as u32, false, true);
        let ntfs_attr = attr.as_ntfs_attr();

        record.add_attribute(NtfsAttribute::standard_info(
            ntfs_attr,
            SECURITY_ID_EVERYONE,
        ));

        record.add_attribute(NtfsAttribute::security_descriptor(
            security_descriptor_everyone(),
        ));

        record.add_attribute(NtfsAttribute::file_name(
            parent_ref,
            name,
            size,
            ntfs_attr,
            NtfsFileNameNamespace::Win32AndDos,
        ));

        if size > 0 {
            let resident_max = (self.meta.mft_record_size as usize).saturating_sub(400);
            if (size as usize) < resident_max {
                let mut data = vec![0u8; size as usize];
                source.read_at(0, &mut data).map_err(FsInjectorError::IO)?;
                record.add_attribute(NtfsAttribute::data_resident(data));
            } else {
                let clusters = size.div_ceil(self.meta.bytes_per_cluster as u64);
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
                record.add_attribute(NtfsAttribute::non_resident(
                    AttributeType::Data,
                    "",
                    self.meta,
                    &handle.runs,
                    size,
                ));
            }
        } else {
            record.add_attribute(NtfsAttribute::data_empty());
        }

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
        );
        ctx.buf.push(entry);

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

            // Skip Root if empty (system files only)
            if mft_num == 5 && ctx.buf.is_empty() {
                return Ok(());
            }

            let parent_ref = if let Some(p) = self.stack.last() {
                build_mft_reference(p.handle.start_lcn, 1)
            } else {
                build_mft_reference(5, 5) // Root self-ref
            };

            let mut record = NtfsMftRecord::new(mft_num as u32, true, true);
            record.add_attribute(NtfsAttribute::standard_info(
                NtfsFileAttributes::DIRECTORY,
                SECURITY_ID_EVERYONE,
            ));
            record.add_attribute(NtfsAttribute::file_name(
                parent_ref,
                ".",
                0,
                NtfsFileAttributes::DIRECTORY,
                NtfsFileNameNamespace::Win32AndDos,
            ));

            record.add_attribute(NtfsAttribute::security_descriptor(
                security_descriptor_everyone(),
            ));

            // Build Index
            let mut entries = ctx.buf;
            // Sort entries (copied from record_builder)
            entries.sort_by(|a, b| {
                let len = a.name.len().min(b.name.len());
                for i in 0..len {
                    let mut ca = a.name[i];
                    if ca >= b'a' as u16 && ca <= b'z' as u16 {
                        ca -= 32;
                    }
                    let mut cb = b.name[i];
                    if cb >= b'a' as u16 && cb <= b'z' as u16 {
                        cb -= 32;
                    }
                    if ca != cb {
                        return ca.cmp(&cb);
                    }
                }
                a.name.len().cmp(&b.name.len())
            });

            let entries_len: usize = entries.iter().map(|e| e.len()).sum();
            let resident_max = (self.meta.mft_record_size as usize).saturating_sub(400);

            if entries_len < resident_max {
                let mut buf = Vec::new();
                for e in &entries {
                    let len = e.len();
                    let old = buf.len();
                    buf.resize(old + len, 0);
                    let mut io = rimio::prelude::MemRimIO::new(&mut buf[old..]);
                    e.write_to_io(&mut io, 0).ok();
                }
                let last_entry = crate::types::IndexEntryHeader::new(0, 0, true);
                buf.extend_from_slice(zerocopy::IntoBytes::as_bytes(&last_entry));

                let clusters_per_index = self.meta.clusters_per_index_record_raw();

                record.add_attribute(NtfsAttribute::index_root_i30(
                    buf,
                    clusters_per_index,
                    self.meta.index_record_size,
                    false,
                ));
            } else {
                let layout =
                    IndexTreeBuilder::build(self.meta, entries).map_err(FsInjectorError::IO)?;

                let clusters_per_index = self.meta.clusters_per_index_record_raw();

                record.add_attribute(NtfsAttribute::index_root_i30(
                    layout.root_entries,
                    clusters_per_index,
                    self.meta.index_record_size,
                    true,
                ));

                let handle = self
                    .allocator
                    .allocate_contiguous(self.io, layout.total_clusters as usize)
                    .map_err(FsInjectorError::Allocator)?;

                let mut phys_runs = rimio::run::RunList::new();
                for run in handle.runs.iter() {
                    phys_runs.push(rimio::run::Run::new(
                        self.meta.lcn_to_offset(run.start),
                        run.length * self.meta.bytes_per_cluster as u64,
                    ));
                }
                let mut mapped = rimio::run::MappedRimIO::new(self.io, &phys_runs, 1);
                let mut off = 0u64;
                for block in layout.allocation_blocks.iter() {
                    mapped.write_at(off, block).map_err(FsInjectorError::IO)?;
                    off += self.meta.index_record_size as u64;
                }

                record.add_attribute(NtfsAttribute::non_resident(
                    AttributeType::IndexAllocation,
                    "$I30",
                    self.meta,
                    &handle.runs,
                    handle.cluster_count() * self.meta.bytes_per_cluster as u64,
                ));
                record.add_attribute(NtfsAttribute::bitmap_named("$I30", layout.bitmap));
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

        // $Bitmap is already updated by allocator.allocate via BitmapDriver

        self.io.flush().map_err(FsInjectorError::IO)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimio::prelude::MemRimIO;

    #[test]
    fn test_ntfs_injector_flow() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, Some("TEST")).unwrap();
        let mut buffer = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        // Full format (VBR + system MFT records)
        crate::formatter::NtfsFormatter::new(&mut io, &meta)
            .format(true)
            .unwrap();

        // Inject
        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Dir {
                    name: "subdir".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::new_file("hello.txt", b"Hello World!".to_vec())],
                },
                FsNode::new_file("readme.md", b"Test Readme".to_vec()),
            ],
        };

        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();
    }
}
