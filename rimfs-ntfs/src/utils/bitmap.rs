// SPDX-License-Identifier: MIT
//! Shared bitmap and bit-array manipulation utilities.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;

use rimio::prelude::*;

pub use crate::core::bitmap::BitmapOps;
use crate::meta::NtfsMeta;

/// Initializes and writes the volume cluster allocation bitmap with all
/// pre-allocated system extents and tail padding bits set to 1.
pub fn write_initial_bitmap<IO: RimIO + ?Sized>(io: &mut IO, meta: &NtfsMeta) -> RimIOResult {
    let bitmap_clusters = meta
        .bitmap_size_bytes
        .div_ceil(meta.bytes_per_cluster as u64) as usize;
    let mut bitmap_data = vec![0u8; bitmap_clusters * meta.bytes_per_cluster as usize];

    // Boot sector clusters (e.g. clusters 0 and 1 if cluster size is 4KB)
    let boot_clusters =
        (16 * meta.bytes_per_sector as u64).div_ceil(meta.bytes_per_cluster as u64) as usize;
    bitmap_data.set_bits_in_range(0, boot_clusters, true);

    // MFT records
    let mft_clusters = (meta.reserved_mft_records * meta.mft_record_size as u64)
        .div_ceil(meta.bytes_per_cluster as u64) as usize;
    let mft_start = meta.mft_lcn as usize;
    bitmap_data.set_bits_in_range(mft_start, mft_start + mft_clusters, true);

    // MFT mirror
    let mirr_clusters =
        (4 * meta.mft_record_size as u64).div_ceil(meta.bytes_per_cluster as u64) as usize;
    let mirr_start = meta.mft_mirr_lcn as usize;
    bitmap_data.set_bits_in_range(mirr_start, mirr_start + mirr_clusters, true);

    // LogFile
    let log_clusters = (2 * 1024 * 1024u64)
        .min(meta.total_clusters * meta.bytes_per_cluster as u64 / 10)
        .div_ceil(meta.bytes_per_cluster as u64) as usize;
    let log_start = meta.logfile_lcn as usize;
    bitmap_data.set_bits_in_range(log_start, log_start + log_clusters, true);

    // Bitmap itself
    let bmp_start = meta.bitmap_lcn as usize;
    bitmap_data.set_bits_in_range(bmp_start, bmp_start + bitmap_clusters, true);

    // Upcase table
    let upcase_clusters = (128 * 1024u64).div_ceil(meta.bytes_per_cluster as u64) as usize;
    let upcase_start = meta.upcase_lcn() as usize;
    bitmap_data.set_bits_in_range(upcase_start, upcase_start + upcase_clusters, true);

    // Mark all tail bits beyond total_clusters as allocated (1) to satisfy Windows CHKDSK
    let total_clusters = meta.total_clusters as usize;
    let total_bits = (meta.bitmap_size_bytes * 8) as usize;
    bitmap_data.set_bits_in_range(total_clusters, total_bits, true);

    let bitmap_offset = meta.lcn_to_offset(meta.bitmap_lcn);
    io.write_at(bitmap_offset, &bitmap_data)?;

    Ok(())
}
