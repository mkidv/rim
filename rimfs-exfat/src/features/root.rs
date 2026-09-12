// SPDX-License-Identifier: MIT

//! exFAT root directory cluster initialization.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::ExFatAllocator;
use crate::core::errors::{FsFeatureError, FsFeatureResult};
use crate::core::fat::FatDriver;
use crate::core::feature::FsSystemFeature;
use crate::core::traits::FsMeta;
use crate::meta::ExFatMeta;
use crate::types::{
    ExFatBitmapEntry, ExFatEodEntry, ExFatGuidEntry, ExFatUpcaseEntry, ExFatVolumeLabelEntry,
};
use crate::upcase::UpcaseHandle;

/// Root directory feature responsible for reserving system clusters in FAT and writing initial root entries.
#[derive(Debug, Default, Clone, Copy)]
pub struct ExFatRootDirFeature;

impl ExFatRootDirFeature {
    pub fn new() -> Self {
        Self
    }
}

impl<'a, IO: RimIO + ?Sized> FsSystemFeature<ExFatMeta, ExFatAllocator<'a>, IO>
    for ExFatRootDirFeature
{
    fn name(&self) -> &str {
        "RootDir"
    }

    fn prepare(&mut self, _meta: &ExFatMeta) -> FsFeatureResult<()> {
        Ok(())
    }

    fn allocate(
        &mut self,
        _io: &mut IO,
        allocator: &mut ExFatAllocator<'a>,
    ) -> FsFeatureResult<()> {
        allocator.used_clusters = allocator.meta.system_used_clusters() as u64;
        Ok(())
    }

    fn write(&self, io: &mut IO, allocator: &ExFatAllocator<'a>) -> FsFeatureResult<()> {
        let meta = allocator.meta;

        // 1. Write the system cluster chains into the FAT table (after FAT format)
        let build_run = |start: u32, len: u32| -> RunList {
            let mut rl = RunList::new();
            rl.push(Run::new(start as u64, len as u64));
            rl
        };

        let bitmap_chain = build_run(meta.bitmap_cluster, meta.bitmap_clusters());
        let upcase_chain = build_run(meta.upcase_cluster, meta.upcase_clusters());
        let root_chain = build_run(meta.root_unit(), meta.root_clusters());

        let mut fd = FatDriver::new(meta);
        for chain in [&bitmap_chain, &upcase_chain, &root_chain] {
            fd.write_run_list(io, chain).map_err(FsFeatureError::from)?;
        }

        // 2. Write the root directory cluster contents
        let upcase = UpcaseHandle::from_flavor(&meta.upcase_flavor);
        let upcase_len = upcase.len() as u64;
        let upcase_checksum = upcase.checksum();

        let mut buf = Vec::with_capacity(meta.unit_size() as usize);

        ExFatBitmapEntry::new(meta.bitmap_cluster, meta.bitmap_size_bytes).to_raw_buffer(&mut buf);
        ExFatUpcaseEntry::new(meta.upcase_cluster, upcase_len, upcase_checksum)
            .to_raw_buffer(&mut buf);
        ExFatVolumeLabelEntry::new(meta.volume_label).to_raw_buffer(&mut buf);
        if let Some(guid) = meta.volume_guid {
            ExFatGuidEntry::new(guid).to_raw_buffer(&mut buf);
        }

        ExFatEodEntry::new().to_raw_buffer(&mut buf);

        let offset = meta.unit_offset(meta.root_unit());
        io.write_at(offset, &buf)?;

        Ok(())
    }
}
