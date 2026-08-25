// SPDX-License-Identifier: MIT
//! NTFS filesystem metadata
//!
//! Contains the computed parameters for an NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::{String, ToString};

pub use crate::core::meta::*;

use rimio::RimIO;
use zerocopy::FromBytes;

use crate::constant::*;
use crate::core::{FsError, FsResult};
use crate::types::NtfsBootSector;
use crate::upcase::UpcaseFlavor;

/// NTFS filesystem metadata
///
/// This structure holds all the computed parameters needed to format
/// and operate on an NTFS volume.
#[derive(Debug, Clone)]
pub struct NtfsMeta {
    /// Volume label (up to 128 UTF-16 code units)
    pub volume_label: [u16; 128],
    /// Volume label length in characters
    pub volume_label_len: u8,

    /// Volume serial number
    pub volume_serial: u64,

    /// Bytes per sector (typically 512)
    pub bytes_per_sector: u16,
    /// Sectors per cluster (power of 2)
    pub sectors_per_cluster: u8,
    /// Bytes per cluster
    pub bytes_per_cluster: u32,

    /// Total volume size in bytes
    pub volume_size_bytes: u64,
    /// Total sectors in volume
    pub total_sectors: u64,
    /// Total clusters in volume
    pub total_clusters: u64,

    /// MFT record size in bytes
    pub mft_record_size: u32,
    /// Index record size in bytes
    pub index_record_size: u32,

    /// LCN (Logical Cluster Number) of $MFT
    pub mft_lcn: u64,
    /// LCN of $MFTMirr
    pub mft_mirr_lcn: u64,

    /// Number of reserved MFT records for system files
    pub reserved_mft_records: u64,

    /// Cluster bitmap size in bytes
    pub bitmap_size_bytes: u64,

    /// Number of hidden sectors (sectors before partition start)
    pub hidden_sectors: u32,

    pub upcase_flavor: UpcaseFlavor,
    /// LCN of $Bitmap
    pub bitmap_lcn: u64,
}

use crate::core::bitmap::BitmapFsMeta;

impl BitmapFsMeta for NtfsMeta {
    fn bitmap_offset(&self) -> u64 {
        self.lcn_to_offset(self.bitmap_lcn)
    }

    fn bitmap_size(&self) -> u64 {
        self.bitmap_size_bytes
    }
}

impl NtfsMeta {
    /// Read NTFS metadata from volume boot sector
    pub fn from_io<IO: RimIO + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let mut boot_buf = [0u8; 512];
        io.read_at(0, &mut boot_buf).map_err(FsError::IO)?;

        let boot = NtfsBootSector::read_from_bytes(&boot_buf)
            .map_err(|_| FsError::Invalid("Failed to read NTFS boot sector"))?;

        let oem_id = boot.oem_id;
        if &oem_id != b"NTFS    " {
            return Err(FsError::Invalid("Not an NTFS volume (OEM ID mismatch)"));
        }

        let end_marker = boot.end_marker;
        if end_marker != 0xAA55 {
            return Err(FsError::Invalid("Invalid boot sector signature"));
        }

        let bytes_per_sector = boot.bytes_per_sector;
        let sectors_per_cluster = boot.sectors_per_cluster;
        if bytes_per_sector == 0 || sectors_per_cluster == 0 {
            return Err(FsError::Invalid("Invalid sector or cluster size"));
        }
        let bytes_per_cluster = boot.bytes_per_cluster();
        let total_sectors = boot.total_sectors;
        let volume_size_bytes = (total_sectors + 1) * bytes_per_sector as u64;
        let total_clusters = total_sectors / sectors_per_cluster as u64;

        let mft_record_size = boot.mft_record_size();
        let index_record_size = boot.index_record_size();

        let mft_lcn = boot.mft_lcn;
        let mft_mirr_lcn = boot.mft_mirr_lcn;

        let bitmap_size_bytes = total_clusters.div_ceil(8);
        let reserved_mft_records = 16;
        let bitmap_lcn = mft_lcn
            + (reserved_mft_records * mft_record_size as u64).div_ceil(bytes_per_cluster as u64);

        Ok(Self {
            volume_label: [0u16; 128],
            volume_label_len: 0,
            volume_serial: boot.volume_serial,
            bytes_per_sector,
            sectors_per_cluster,
            bytes_per_cluster,
            volume_size_bytes,
            total_sectors,
            total_clusters,
            mft_record_size,
            index_record_size,
            mft_lcn,
            mft_mirr_lcn,
            reserved_mft_records,
            bitmap_size_bytes,
            hidden_sectors: 0,
            upcase_flavor: UpcaseFlavor::Legacy,
            bitmap_lcn,
        })
    }

    /// Create new NTFS metadata for the given volume size
    pub fn new(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let cluster_size = determine_cluster_size(size_bytes);
        Self::new_custom(
            size_bytes,
            volume_label,
            None,
            NTFS_SECTOR_SIZE,
            cluster_size,
            NTFS_MFT_RECORD_SIZE,
            NTFS_INDEX_RECORD_SIZE,
            0, // Default to 0, though rimgen should pass it
            UpcaseFlavor::Legacy,
        )
    }

    /// Create NTFS metadata with custom parameters
    #[allow(clippy::too_many_arguments)]
    pub fn new_custom(
        volume_size_bytes: u64,
        volume_label: Option<&str>,
        volume_serial: Option<u64>,
        bytes_per_sector: u16,
        bytes_per_cluster: u32,
        mft_record_size: u32,
        index_record_size: u32,
        hidden_sectors: u32,
        upcase_flavor: UpcaseFlavor,
    ) -> FsResult<Self> {
        // Validate parameters
        if bytes_per_cluster < bytes_per_sector as u32 {
            return Err(FsError::Invalid("cluster size must be >= sector size"));
        }
        if !bytes_per_cluster.is_power_of_two() {
            return Err(FsError::Invalid("cluster size must be power of 2"));
        }
        if !mft_record_size.is_power_of_two() {
            return Err(FsError::Invalid("MFT record size must be power of 2"));
        }

        let sectors_per_cluster = (bytes_per_cluster / bytes_per_sector as u32) as u8;
        // BPB_TotSec64 must be PartitionSectors - 1 (the last sector is reserved for backup VBR)
        let total_sectors = (volume_size_bytes / bytes_per_sector as u64) - 1;
        // Total clusters covers only the active volume area
        let total_clusters = total_sectors / sectors_per_cluster as u64;

        // Parse volume label
        let mut label_buf = [0u16; 128];
        let mut label_len = 0u8;
        if let Some(label) = volume_label {
            for (i, ch) in label.encode_utf16().take(128).enumerate() {
                label_buf[i] = ch;
                label_len = (i + 1) as u8;
            }
        }

        // Generate or use provided serial
        let serial = volume_serial.unwrap_or_else(|| {
            derive_volume_serial(
                volume_label.unwrap_or(""),
                volume_size_bytes,
                bytes_per_cluster,
            )
        });

        // Bitmap size: 1 bit per cluster
        let bitmap_size_bytes = total_clusters.div_ceil(8);

        // Calculate system file range to avoid MFT overlap
        let log_clusters = (2 * 1024 * 1024)
            .min(volume_size_bytes / 10)
            .div_ceil(bytes_per_cluster as u64);
        let attrdef_clusters = 1;
        let root_index_clusters = 1;
        let bitmap_clusters = bitmap_size_bytes.div_ceil(bytes_per_cluster as u64);
        let upcase_clusters = (128 * 1024u64).div_ceil(bytes_per_cluster as u64);

        let bitmap_lcn = 3 + log_clusters + attrdef_clusters + root_index_clusters;
        // Skip bootstrap (0-1), MFTMirr (2), and system files
        let first_system_data = bitmap_lcn + bitmap_clusters + upcase_clusters;

        // Calculate MFT position
        let mft_lcn = calculate_mft_lcn(total_clusters, first_system_data);

        // MFTMirr is at cluster 2 in standard Windows formatting for better compatibility
        let mft_mirr_lcn = 2;

        Ok(Self {
            volume_label: label_buf,
            volume_label_len: label_len,
            volume_serial: serial,
            bytes_per_sector,
            sectors_per_cluster,
            bytes_per_cluster,
            volume_size_bytes,
            total_sectors,
            total_clusters,
            mft_record_size,
            index_record_size,
            mft_lcn,
            mft_mirr_lcn,
            reserved_mft_records: NTFS_RESERVED_MFT_RECORDS,
            bitmap_size_bytes,
            hidden_sectors,
            upcase_flavor,
            bitmap_lcn,
        })
    }

    /// Get volume label as string
    pub fn label_string(&self) -> String {
        String::from_utf16_lossy(&self.volume_label[..self.volume_label_len as usize])
    }

    /// Calculate offset in bytes for a given LCN
    #[inline]
    pub fn lcn_to_offset(&self, lcn: u64) -> u64 {
        lcn * self.bytes_per_cluster as u64
    }

    /// Calculate offset for a given MFT record number
    #[inline]
    pub fn mft_record_offset(&self, record_number: u64) -> u64 {
        self.lcn_to_offset(self.mft_lcn) + record_number * self.mft_record_size as u64
    }

    /// Calculate how many clusters are needed for the initial MFT
    pub fn initial_mft_clusters(&self) -> u64 {
        let mft_bytes = self.reserved_mft_records * self.mft_record_size as u64;
        mft_bytes.div_ceil(self.bytes_per_cluster as u64)
    }
}

impl FsMeta<u64> for NtfsMeta {
    fn unit_size(&self) -> usize {
        self.bytes_per_cluster as usize
    }

    fn root_unit(&self) -> u64 {
        // Root directory is MFT record 5, but for cluster allocation
        // we return the MFT LCN as the "root" unit
        self.mft_lcn
    }

    fn total_units(&self) -> usize {
        self.total_clusters as usize
    }

    fn size_bytes(&self) -> u64 {
        self.volume_size_bytes
    }

    fn label(&self) -> String {
        self.label_string()
    }

    fn unit_offset(&self, lcn: u64) -> u64 {
        self.lcn_to_offset(lcn)
    }

    fn first_data_unit(&self) -> u64 {
        // System files are allocated sequentially during format:
        // skip clusters 0-1 (boot area) and cluster 2 ($MFTMirr)
        let log_clusters = (2 * 1024 * 1024)
            .min(self.volume_size_bytes / 10)
            .div_ceil(self.bytes_per_cluster as u64);
        let attrdef_clusters = 1;
        let root_index_clusters = 1;
        let bitmap_clusters = self
            .bitmap_size_bytes
            .div_ceil(self.bytes_per_cluster as u64);
        let upcase_clusters = (128 * 1024u64).div_ceil(self.bytes_per_cluster as u64);

        3 + log_clusters
            + attrdef_clusters
            + root_index_clusters
            + bitmap_clusters
            + upcase_clusters
    }

    fn last_data_unit(&self) -> u64 {
        self.total_clusters - 1
    }
}

/// Determines optimal cluster size based on volume size
fn determine_cluster_size(size_bytes: u64) -> u32 {
    const GB: u64 = 1024 * 1024 * 1024;

    // Microsoft recommended cluster sizes for NTFS
    match size_bytes {
        0..=536_870_912 => 512,                // <= 512 MB: 512 bytes
        536_870_913..=1_073_741_824 => 1024,   // 512 MB - 1 GB: 1 KB
        1_073_741_825..=2_147_483_648 => 2048, // 1 GB - 2 GB: 2 KB
        _ if size_bytes <= 16 * GB => 4096,    // 2 GB - 16 GB: 4 KB (default)
        _ if size_bytes <= 32 * GB => 8192,    // 16 GB - 32 GB: 8 KB
        _ if size_bytes <= 64 * GB => 16384,   // 32 GB - 64 GB: 16 KB
        _ if size_bytes <= 128 * GB => 32768,  // 64 GB - 128 GB: 32 KB
        _ => 65536,                            // > 128 GB: 64 KB
    }
}

/// Calculate MFT starting LCN
fn calculate_mft_lcn(total_clusters: u64, min_lcn: u64) -> u64 {
    // MFT is typically placed at ~12.5% of the volume, or a fixed location
    // For simplicity, we place it a bit after the start
    let target = total_clusters / 8; // ~12.5%
    // Ensure minimum offset to avoid overlapping with system files
    target.max(min_lcn)
}

/// Derive a deterministic volume serial from label and size
fn derive_volume_serial(label: &str, size_bytes: u64, cluster_size: u32) -> u64 {
    use crate::core::utils::volume::derive_ids;
    let (_, vol_id) = derive_ids(label, size_bytes, cluster_size, 0);
    // Extend to 64-bit by combining with size component
    let size_component = size_bytes >> 20; // MB component
    ((vol_id as u64) << 32) | size_component
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meta_creation() {
        let meta = NtfsMeta::new(100 * 1024 * 1024, Some("TESTNTFS")).unwrap();

        assert_eq!(meta.bytes_per_sector, 512);
        assert!(meta.bytes_per_cluster >= 512);
        assert!(meta.mft_lcn > 0);
        assert!(meta.total_clusters > 0);
        assert_eq!(meta.label_string(), "TESTNTFS");
    }

    #[test]
    fn test_cluster_size_selection() {
        // Small volume
        let small = NtfsMeta::new(256 * 1024 * 1024, None).unwrap();
        assert!(small.bytes_per_cluster <= 1024);

        // Medium volume
        let medium = NtfsMeta::new(4 * 1024 * 1024 * 1024, None).unwrap();
        assert_eq!(medium.bytes_per_cluster, 4096);

        // Large volume
        let large = NtfsMeta::new(200 * 1024 * 1024 * 1024, None).unwrap();
        assert!(large.bytes_per_cluster >= 32768);
    }
}
