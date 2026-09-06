// SPDX-License-Identifier: MIT
//! NTFS Formatter
//!
//! Orchestrates NTFS filesystem creation using the modular `FsSystemFeature` architecture,
//! exactly aligned with `rimfs-ext` and `rimfs-core`.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::boxed::Box;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::NtfsAllocator;
use crate::core::feature::FsSystemFeature;
use crate::core::{FsFormatterError, FsFormatterResult, traits::FsFormatter};
use crate::features::{
    NtfsAttrDefFeature, NtfsBadClusFeature, NtfsBootFeature, NtfsExtendFeature, NtfsLogFileFeature,
    NtfsMftFeature, NtfsRootDirFeature, NtfsSecureFeature, NtfsUpCaseFeature, NtfsVolumeFeature,
};
use crate::meta::NtfsMeta;
use crate::utils::current_ntfs_time;

/// NTFS filesystem formatter
pub struct NtfsFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a NtfsMeta,
}

impl<'a, IO: RimIO + ?Sized> NtfsFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> Self {
        Self { io, meta }
    }

    /// Format the filesystem (forwarding to `FsFormatter::format`).
    pub fn format(&mut self, full_format: bool) -> FsFormatterResult<()> {
        FsFormatter::format(self, full_format)
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for NtfsFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult<()> {
        if full_format {
            let _ = crate::core::formatter::zero_cluster_heap(self.io, self.meta);
        }

        // 1. Initialize on-disk cluster allocation bitmap with all system reservations (exFAT-aligned pattern)
        crate::utils::write_initial_bitmap(self.io, self.meta).map_err(FsFormatterError::from)?;

        // Synchronized timestamp across all initial system files
        let timestamp = current_ntfs_time();

        let mut features: Vec<Box<dyn FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>>> = vec![
            Box::new(NtfsBootFeature::new()),
            Box::new(NtfsMftFeature::new(timestamp)),
            Box::new(NtfsLogFileFeature::new()),
            Box::new(NtfsVolumeFeature::new()),
            Box::new(NtfsAttrDefFeature::new()),
            Box::new(NtfsRootDirFeature::new(timestamp)),
            Box::new(NtfsBadClusFeature::new()),
            Box::new(NtfsSecureFeature::new(timestamp)),
            Box::new(NtfsUpCaseFeature::new()),
            Box::new(NtfsExtendFeature::new(timestamp)),
        ];

        let mut allocator = NtfsAllocator::new(self.meta)?;

        // Phase 1: Prepare & Phase 2: Allocate
        for feature in &mut features {
            feature.prepare(self.meta).map_err(FsFormatterError::from)?;
            feature
                .allocate(self.io, &mut allocator)
                .map_err(FsFormatterError::from)?;
        }

        // Phase 3: Write
        for feature in &mut features {
            feature
                .write(self.io, &allocator)
                .map_err(FsFormatterError::from)?;
        }

        // Final sync of MFT Mirror to ensure primary records 0..3 are bit-for-bit identical
        let mirror_offset = self.meta.lcn_to_offset(self.meta.mft_mirr_lcn);
        let mft_offset = self.meta.lcn_to_offset(self.meta.mft_lcn);
        let mirror_size = if self.meta.bytes_per_cluster <= 4096 {
            4 * self.meta.mft_record_size as usize
        } else {
            self.meta.bytes_per_cluster as usize
        };
        let mut mirror_data = vec![0u8; mirror_size];
        self.io.read_at(mft_offset, &mut mirror_data)?;
        self.io.write_at(mirror_offset, &mirror_data)?;

        self.io.flush()?;
        Ok(())
    }
}
