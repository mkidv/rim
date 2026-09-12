// SPDX-License-Identifier: MIT
//! NTFS Root Directory Feature (.)
//!
//! Handles writing Inode 5 (the root directory `.`), its $I30 index root,
//! index allocation blocks if needed, and the 228-byte Windows canonical
//! security descriptor granting full access to Administrators/SYSTEM and
//! read/execute access to Users.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::{FsAllocator, NtfsAllocator, NtfsHandle};
use crate::attr::NtfsFileAttributes;
use crate::constant::*;
use crate::core::errors::FsFeatureResult;
use crate::core::feature::FsSystemFeature;
use crate::meta::NtfsMeta;
use crate::types::index::NtfsIndexEntry;
use crate::types::security::SECURITY_DESCRIPTOR_ROOT;
use crate::types::{
    IndexTreeBuilder, NtfsAttribute, NtfsAttributeType, NtfsFileNameNamespace, NtfsMftRecord,
};
use crate::upcase::UpcaseHandle;

pub struct NtfsRootDirFeature {
    timestamp: u64,
    non_resident_handle: Option<NtfsHandle>,
}

impl NtfsRootDirFeature {
    pub fn new(timestamp: u64) -> Self {
        Self {
            timestamp,
            non_resident_handle: None,
        }
    }

    pub fn root_ref() -> u64 {
        crate::mft::build_mft_reference(MFT_RECORD_ROOT, 5)
    }

    pub fn build_root_entries(meta: &NtfsMeta, timestamp: u64) -> Vec<NtfsIndexEntry> {
        let root_ref = Self::root_ref();

        let mft_size = meta.initial_mft_clusters() * meta.bytes_per_cluster as u64;
        let mftmirr_size = (4u64 * meta.mft_record_size as u64)
            .div_ceil(meta.bytes_per_cluster as u64)
            * meta.bytes_per_cluster as u64;
        let log_size = (2 * 1024 * 1024u64).min(meta.volume_size_bytes / 10);
        let attr_def_size = crate::types::attrdef::build_standard_attr_defs().len() as u64;
        let bitmap_size = meta.total_clusters.div_ceil(8);
        let boot_size = 16 * meta.bytes_per_sector as u64;
        let upcase_size = UpcaseHandle::from_flavor(&meta.upcase_flavor)
            .as_bytes()
            .len() as u64;
        let sys = NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM;
        let sys_dir = sys | NtfsFileAttributes::I30_INDEX;
        let sys_view = sys | NtfsFileAttributes::VIEW_INDEX;

        struct RootSpec {
            rec: u64,
            name: &'static str,
            attrs: NtfsFileAttributes,
            data_size: u64,
            allocated_size: u64,
        }

        let alloc = |size: u64| {
            size.div_ceil(meta.bytes_per_cluster as u64) * meta.bytes_per_cluster as u64
        };
        let items = [
            RootSpec {
                rec: MFT_RECORD_MFT,
                name: "$MFT",
                attrs: sys,
                data_size: mft_size,
                allocated_size: mft_size,
            },
            RootSpec {
                rec: MFT_RECORD_MFTMIRR,
                name: "$MFTMirr",
                attrs: sys,
                data_size: mftmirr_size,
                allocated_size: mftmirr_size,
            },
            RootSpec {
                rec: MFT_RECORD_LOGFILE,
                name: "$LogFile",
                attrs: sys,
                data_size: log_size,
                allocated_size: alloc(log_size),
            },
            RootSpec {
                rec: MFT_RECORD_VOLUME,
                name: "$Volume",
                attrs: sys,
                data_size: 0,
                allocated_size: 0,
            },
            RootSpec {
                rec: MFT_RECORD_ATTRDEF,
                name: "$AttrDef",
                attrs: sys,
                data_size: attr_def_size,
                allocated_size: if attr_def_size < 600 {
                    attr_def_size
                } else {
                    alloc(attr_def_size)
                },
            },
            RootSpec {
                rec: MFT_RECORD_BITMAP,
                name: "$Bitmap",
                attrs: sys,
                data_size: bitmap_size,
                allocated_size: alloc(bitmap_size),
            },
            RootSpec {
                rec: MFT_RECORD_BOOT,
                name: "$Boot",
                attrs: sys,
                data_size: boot_size,
                allocated_size: alloc(boot_size),
            },
            RootSpec {
                rec: MFT_RECORD_BADCLUS,
                name: "$BadClus",
                attrs: sys,
                data_size: 0,
                allocated_size: 0,
            },
            RootSpec {
                rec: MFT_RECORD_SECURE,
                name: "$Secure",
                attrs: sys_view,
                data_size: 0,
                allocated_size: 0,
            },
            RootSpec {
                rec: MFT_RECORD_UPCASE,
                name: "$UpCase",
                attrs: sys,
                data_size: upcase_size,
                allocated_size: alloc(upcase_size),
            },
            RootSpec {
                rec: MFT_RECORD_EXTEND,
                name: "$Extend",
                attrs: sys_dir,
                data_size: 0,
                allocated_size: 0,
            },
            RootSpec {
                rec: MFT_RECORD_ROOT,
                name: ".",
                attrs: sys_dir,
                data_size: 0,
                allocated_size: 0,
            },
        ];

        items
            .into_iter()
            .map(|item| {
                NtfsIndexEntry::new(
                    crate::mft::system_file_mft_reference(item.rec),
                    root_ref,
                    item.name.encode_utf16().collect(),
                    item.attrs,
                    crate::flags::IndexEntryFlags::empty(),
                    None,
                )
                .with_timestamps(timestamp)
                .with_namespace(NtfsFileNameNamespace::Win32AndDos)
                .with_sizes(item.data_size, item.allocated_size)
            })
            .collect()
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>
    for NtfsRootDirFeature
{
    fn name(&self) -> &str {
        "NTFS Root Directory (.)"
    }

    fn prepare(&mut self, _meta: &NtfsMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(&mut self, io: &mut IO, allocator: &mut NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let entries = Self::build_root_entries(meta, self.timestamp);

        if let crate::types::DirectoryIndexResult::NonResident { layout } =
            IndexTreeBuilder::build_directory_index(meta, entries).map_err(|_| {
                crate::core::errors::FsFeatureError::Other("Index layout build failed")
            })?
        {
            let handle = allocator
                .allocate_contiguous(io, layout.total_clusters)
                .map_err(crate::core::errors::FsFeatureError::Allocator)?;
            self.non_resident_handle = Some(handle);
        }

        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &NtfsAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;
        let entries = Self::build_root_entries(meta, self.timestamp);
        let index_res = IndexTreeBuilder::build_directory_index(meta, entries)
            .map_err(|_| crate::core::errors::FsFeatureError::Other("Index layout build failed"))?;

        let (index_entries, has_children, non_resident) = match index_res {
            crate::types::DirectoryIndexResult::Resident { entries_buf } => {
                (entries_buf, false, None)
            }
            crate::types::DirectoryIndexResult::NonResident { layout } => {
                (layout.root_entries.clone(), true, Some(layout))
            }
        };

        let mut record = NtfsMftRecord::new(MFT_RECORD_ROOT as u32, true, true);
        let attrs =
            NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::DIRECTORY;
        record.add_attribute(NtfsAttribute::standard_info_basic(attrs, self.timestamp));
        record.add_attribute(NtfsAttribute::file_name_custom(
            Self::root_ref(),
            ".",
            0,
            (attrs - NtfsFileAttributes::DIRECTORY) | NtfsFileAttributes::I30_INDEX,
            NtfsFileNameNamespace::Win32AndDos,
            self.timestamp,
        ));
        record.add_attribute(NtfsAttribute::security_descriptor(
            SECURITY_DESCRIPTOR_ROOT.to_bytes(),
        ));
        let clusters_per_index = meta.clusters_per_index_record_raw();
        record.add_attribute(NtfsAttribute::index_root_i30(
            index_entries,
            clusters_per_index,
            meta.index_record_size,
            has_children,
        ));

        if let Some(layout) = non_resident {
            let handle = self.non_resident_handle.as_ref().ok_or(
                crate::core::errors::FsFeatureError::InvalidConfiguration(
                    "Index blocks not allocated",
                ),
            )?;

            layout
                .write_allocation_blocks(io, meta, &handle.runs)
                .map_err(crate::core::errors::FsFeatureError::IO)?;

            record.add_attribute(NtfsAttribute::non_resident(
                NtfsAttributeType::IndexAllocation,
                "$I30",
                meta,
                &handle.runs,
                handle.cluster_count() * meta.bytes_per_cluster as u64,
            ));
            record.add_attribute(NtfsAttribute::bitmap_named("$I30", layout.bitmap));
        }

        record
            .write_to_mft(io, meta, MFT_RECORD_ROOT)
            .map_err(crate::core::errors::FsFeatureError::IO)?;

        Ok(())
    }
}
