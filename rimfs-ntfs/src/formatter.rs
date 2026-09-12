// SPDX-License-Identifier: MIT

//! NTFS volume formatter and system file generator.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::allocator::NtfsAllocator;
use crate::core::bitmap::BitmapDriver;
use crate::core::feature::{FsSystemFeature, execute_feature_pipeline};
use crate::core::{FsFormatterError, FsFormatterResult, traits::FsFormatter};
use crate::features::{
    NtfsAttrDefFeature, NtfsBadClusFeature, NtfsBootFeature, NtfsExtendFeature, NtfsLogFileFeature,
    NtfsMftFeature, NtfsRootDirFeature, NtfsSecureFeature, NtfsUpCaseFeature, NtfsVolumeFeature,
};
use crate::meta::*;
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

    /// Initializes and writes the volume cluster allocation bitmap with all
    /// pre-allocated system extents and tail padding bits set to 1.
    fn write_initial_bitmap(&mut self) -> FsFormatterResult<()> {
        let mut driver = BitmapDriver::new(self.meta);
        driver
            .format_with(self.io, 0x00)
            .map_err(FsFormatterError::IO)?;

        // Boot sector clusters (e.g. clusters 0 and 1 if cluster size is 4KB)
        let boot_clusters =
            (16 * self.meta.bytes_per_sector as u64).div_ceil(self.meta.bytes_per_cluster as u64);
        driver
            .set_bits_range(self.io, 0, boot_clusters, true)
            .map_err(FsFormatterError::IO)?;

        // MFT records
        let mft_clusters = (self.meta.reserved_mft_records * self.meta.mft_record_size as u64)
            .div_ceil(self.meta.bytes_per_cluster as u64);
        let mft_start = self.meta.mft_lcn;
        driver
            .set_bits_range(self.io, mft_start, mft_clusters, true)
            .map_err(FsFormatterError::IO)?;

        // MFT mirror
        let mirr_clusters =
            (4 * self.meta.mft_record_size as u64).div_ceil(self.meta.bytes_per_cluster as u64);
        let mirr_start = self.meta.mft_mirr_lcn;
        driver
            .set_bits_range(self.io, mirr_start, mirr_clusters, true)
            .map_err(FsFormatterError::IO)?;

        // LogFile
        let log_clusters = (2 * 1024 * 1024u64)
            .min(self.meta.total_clusters * self.meta.bytes_per_cluster as u64 / 10)
            .div_ceil(self.meta.bytes_per_cluster as u64);
        let log_start = self.meta.logfile_lcn;
        driver
            .set_bits_range(self.io, log_start, log_clusters, true)
            .map_err(FsFormatterError::IO)?;

        // Bitmap itself
        let bitmap_clusters = self
            .meta
            .bitmap_size_bytes
            .div_ceil(self.meta.bytes_per_cluster as u64);
        let bmp_start = self.meta.bitmap_lcn;
        driver
            .set_bits_range(self.io, bmp_start, bitmap_clusters, true)
            .map_err(FsFormatterError::IO)?;

        // Upcase table
        let upcase_clusters = (128 * 1024u64).div_ceil(self.meta.bytes_per_cluster as u64);
        let upcase_start = self.meta.upcase_lcn();
        driver
            .set_bits_range(self.io, upcase_start, upcase_clusters, true)
            .map_err(FsFormatterError::IO)?;

        // Mark all tail bits beyond total_clusters as allocated (1) to satisfy Windows CHKDSK
        let total_clusters = self.meta.total_clusters;
        let total_bits = self.meta.bitmap_size_bytes * 8;
        if total_bits > total_clusters {
            driver
                .set_bits_range(self.io, total_clusters, total_bits - total_clusters, true)
                .map_err(FsFormatterError::IO)?;
        }

        driver.flush(self.io).map_err(FsFormatterError::IO)?;

        // Also zero out any slack space in the last cluster of the bitmap file if needed
        let allocated_bytes = bitmap_clusters * self.meta.bytes_per_cluster as u64;
        if allocated_bytes > self.meta.bitmap_size_bytes {
            self.io
                .zero_at(
                    self.meta.lcn_to_offset(self.meta.bitmap_lcn) + self.meta.bitmap_size_bytes,
                    allocated_bytes - self.meta.bitmap_size_bytes,
                )
                .map_err(FsFormatterError::IO)?;
        }

        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for NtfsFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult<()> {
        if full_format {
            crate::core::formatter::zero_cluster_heap(self.io, self.meta)?;
        }

        self.write_initial_bitmap()?;

        let timestamp = current_ntfs_time();

        let mut boot = NtfsBootFeature::new();
        let mut mft = NtfsMftFeature::new(timestamp);
        let mut log_file = NtfsLogFileFeature::new();
        let mut volume = NtfsVolumeFeature::new();
        let mut attr_def = NtfsAttrDefFeature::new();
        let mut root_dir = NtfsRootDirFeature::new(timestamp);
        let mut bad_clus = NtfsBadClusFeature::new();
        let mut secure = NtfsSecureFeature::new(timestamp);
        let mut upcase = NtfsUpCaseFeature::new();
        let mut extend = NtfsExtendFeature::new(timestamp);

        let mut features: [&mut dyn FsSystemFeature<NtfsMeta, NtfsAllocator<'a>, IO>; 10] = [
            &mut boot,
            &mut mft,
            &mut log_file,
            &mut volume,
            &mut attr_def,
            &mut root_dir,
            &mut bad_clus,
            &mut secure,
            &mut upcase,
            &mut extend,
        ];

        let mut allocator = NtfsAllocator::new(self.meta)?;
        execute_feature_pipeline(&mut features, self.meta, &mut allocator, self.io)?;

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
