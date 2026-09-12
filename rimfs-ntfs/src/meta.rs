// SPDX-License-Identifier: MIT
//! NTFS filesystem metadata
//!
//! Contains the computed parameters for an NTFS volume.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::String;
use rimfs_core::bitmap::BitmapFsMeta;

pub use crate::core::meta::*;

use rimio::RimReadStructExt;

use crate::constant::*;
use crate::core::{FsError, FsResult};
use crate::types::{NtfsAttributeType, NtfsBootSector};
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
    /// LCN of $LogFile
    pub logfile_lcn: u64,
    /// LCN of $Bitmap
    pub bitmap_lcn: u64,
}

impl NtfsMeta {
    /// Read NTFS metadata from volume boot sector
    pub fn from_io<IO: rimio::RimRead + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let boot: NtfsBootSector = io.read_struct(0).map_err(FsError::IO)?;

        let oem_id = boot.oem_id;
        crate::ensure!(
            oem_id == NTFS_BOOT_SIGNATURE,
            FsError::Invalid("Not an NTFS volume (OEM ID mismatch)")
        );

        let end_marker = boot.end_marker.get();
        crate::ensure!(
            end_marker == 0xAA55,
            FsError::Invalid("Invalid boot sector signature")
        );

        let bytes_per_sector = boot.bytes_per_sector.get();
        let sectors_per_cluster = boot.sectors_per_cluster;
        crate::ensure!(
            bytes_per_sector > 0 && sectors_per_cluster > 0,
            FsError::Invalid("Invalid sector or cluster size")
        );
        let bytes_per_cluster = boot.bytes_per_cluster();
        let total_sectors = boot.total_sectors.get();
        let volume_size_bytes = (total_sectors + 1) * bytes_per_sector as u64;
        let total_clusters = total_sectors / sectors_per_cluster as u64;

        let mft_record_size = boot.mft_record_size();
        let index_record_size = boot.index_record_size();

        crate::ensure!(
            mft_record_size >= bytes_per_sector as u32
                && index_record_size >= bytes_per_sector as u32,
            FsError::Invalid("Invalid NTFS record size")
        );

        let mft_lcn = boot.mft_lcn.get();
        let mft_mirr_lcn = boot.mft_mirr_lcn.get();

        let bitmap_size_bytes = total_clusters.div_ceil(8);
        let mirr_clusters = (4 * mft_record_size as u64).div_ceil(bytes_per_cluster as u64);
        let mut logfile_lcn = mft_mirr_lcn + mirr_clusters;
        let log_clusters = (2 * 1024 * 1024u64)
            .min(volume_size_bytes / 10)
            .div_ceil(bytes_per_cluster as u64);
        let mut bitmap_lcn = logfile_lcn + log_clusters;
        let mut reserved_mft_records = crate::constant::NTFS_RESERVED_MFT_RECORDS;

        // Try to read actual locations and sizes from MFT records 0 ($MFT), 2 ($LogFile), and 6 ($Bitmap)
        let mft_base = mft_lcn * bytes_per_cluster as u64;
        let mut rec_buf = alloc::vec![0u8; mft_record_size as usize];

        if io.read_at(mft_base, &mut rec_buf).is_ok()
            && crate::utils::decode_usa_fixup(&mut rec_buf, bytes_per_sector as usize)
            && let Ok(view0) = crate::view::mft_view::MftRecordView::new(&rec_buf)
            && let Ok(Some(data_attr)) = view0.find(NtfsAttributeType::Data)
            && let Ok(crate::view::attr_view::AttrView::NonResident { allocated_size, .. }) =
                data_attr.as_view()
        {
            let count = allocated_size / mft_record_size as u64;
            if count > 0 {
                reserved_mft_records = count;
            }
        }

        if io
            .read_at(mft_base + 2 * mft_record_size as u64, &mut rec_buf)
            .is_ok()
            && crate::utils::decode_usa_fixup(&mut rec_buf, bytes_per_sector as usize)
            && let Ok(view2) = crate::view::mft_view::MftRecordView::new(&rec_buf)
            && let Ok(Some(data_attr)) = view2.find(NtfsAttributeType::Data)
            && let Ok(crate::view::attr_view::AttrView::NonResident { runlist, .. }) =
                data_attr.as_view()
            && let Some(lcn) = runlist.iter().find_map(|r| r.lcn)
        {
            logfile_lcn = lcn;
        }

        if io
            .read_at(mft_base + 6 * mft_record_size as u64, &mut rec_buf)
            .is_ok()
            && crate::utils::decode_usa_fixup(&mut rec_buf, bytes_per_sector as usize)
            && let Ok(view6) = crate::view::mft_view::MftRecordView::new(&rec_buf)
            && let Ok(Some(data_attr)) = view6.find(NtfsAttributeType::Data)
            && let Ok(crate::view::attr_view::AttrView::NonResident { runlist, .. }) =
                data_attr.as_view()
            && let Some(lcn) = runlist.iter().find_map(|r| r.lcn)
        {
            bitmap_lcn = lcn;
        }

        Ok(Self {
            volume_label: [0u16; 128],
            volume_label_len: 0,
            volume_serial: boot.volume_serial.get(),
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
            upcase_flavor: UpcaseFlavor::Windows,
            logfile_lcn,
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
            UpcaseFlavor::Windows,
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
        crate::ensure!(
            bytes_per_cluster >= bytes_per_sector as u32,
            FsError::Invalid("cluster size must be >= sector size")
        );
        crate::ensure!(
            bytes_per_cluster.is_power_of_two(),
            FsError::Invalid("cluster size must be power of 2")
        );
        crate::ensure!(
            mft_record_size.is_power_of_two(),
            FsError::Invalid("MFT record size must be power of 2")
        );

        let sectors_per_cluster = (bytes_per_cluster / bytes_per_sector as u32) as u8;
        // BPB_TotSec64 must be PartitionSectors - 1 (the last sector is reserved for backup VBR)
        let total_sectors = (volume_size_bytes / bytes_per_sector as u64) - 1;
        // Total clusters covers only the active volume area
        let total_clusters = total_sectors / sectors_per_cluster as u64;

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

        let boot_clusters = (16 * bytes_per_sector as u64).div_ceil(bytes_per_cluster as u64);
        let mirr_clusters = (4 * mft_record_size as u64).div_ceil(bytes_per_cluster as u64);
        // Keep $MFTMirr out of the $Boot file extent (first 16 sectors).
        let mft_mirr_lcn = 2.max(boot_clusters);
        // LogFile starts immediately after MFTMirr clusters
        let logfile_lcn = mft_mirr_lcn + mirr_clusters;

        let log_clusters = (2 * 1024 * 1024)
            .min(volume_size_bytes / 10)
            .div_ceil(bytes_per_cluster as u64);
        let bitmap_clusters = bitmap_size_bytes.div_ceil(bytes_per_cluster as u64);
        let upcase_clusters = (128 * 1024u64).div_ceil(bytes_per_cluster as u64);

        let bitmap_lcn = logfile_lcn + log_clusters;
        let first_system_data = bitmap_lcn + bitmap_clusters + upcase_clusters;

        let mft_lcn = calculate_mft_lcn(total_clusters, first_system_data);

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
            logfile_lcn,
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

    /// Encoded clusters per index record field as used in the BPB and $INDEX_ROOT header.
    /// Positive value indicates number of clusters; negative value indicates 2^(-val) bytes.
    pub fn clusters_per_index_record_raw(&self) -> i8 {
        if self.index_record_size >= self.bytes_per_cluster {
            (self.index_record_size / self.bytes_per_cluster) as i8
        } else {
            -(self.index_record_size.trailing_zeros() as i8)
        }
    }

    /// Offset in bytes of the backup (alternate) boot sector.
    /// The NTFS backup boot sector is located in the final sector of the volume (sector total_sectors).
    pub fn backup_boot_sector_offset(&self) -> u64 {
        self.total_sectors * self.bytes_per_sector as u64
    }

    /// Calculate the Virtual Cluster Number (VCN) for a given index block index (0-indexed).
    /// If index_record_size >= bytes_per_cluster, VCN is in filesystem cluster units.
    /// If index_record_size < bytes_per_cluster, VCN is in 512-byte sectors per NTFS specification.
    pub fn index_block_to_vcn(&self, block_index: u64) -> u64 {
        if self.index_record_size >= self.bytes_per_cluster {
            block_index * (self.index_record_size / self.bytes_per_cluster) as u64
        } else {
            (block_index * self.index_record_size as u64) / self.bytes_per_sector as u64
        }
    }

    /// Convert a VCN back to an index block index (0-indexed).
    pub fn vcn_to_index_block(&self, vcn: u64) -> Option<u64> {
        let vcn_per_block = self.index_block_to_vcn(1);
        if vcn_per_block == 0 {
            return None;
        }
        if vcn.is_multiple_of(vcn_per_block) {
            Some(vcn / vcn_per_block)
        } else {
            None
        }
    }

    /// Total clusters needed to allocate N index blocks in $INDEX_ALLOCATION.
    pub fn total_clusters_for_index_blocks(&self, block_count: usize) -> u64 {
        let total_bytes = block_count as u64 * self.index_record_size as u64;
        total_bytes.div_ceil(self.bytes_per_cluster as u64)
    }

    #[inline]
    pub fn upcase_lcn(&self) -> u64 {
        let bitmap_clusters = self
            .bitmap_size_bytes
            .div_ceil(self.bytes_per_cluster as u64);
        self.bitmap_lcn + bitmap_clusters
    }
}

impl FsMeta<u64> for NtfsMeta {
    fn unit_size(&self) -> u64 {
        self.bytes_per_cluster as u64
    }

    fn root_unit(&self) -> u64 {
        // Root directory is MFT record 5, but for cluster allocation
        // we return the MFT LCN as the "root" unit
        self.mft_lcn
    }

    fn total_units(&self) -> u64 {
        self.total_clusters
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
        let bitmap_clusters = self
            .bitmap_size_bytes
            .div_ceil(self.bytes_per_cluster as u64);
        let upcase_clusters = (128 * 1024u64).div_ceil(self.bytes_per_cluster as u64);
        self.bitmap_lcn + bitmap_clusters + upcase_clusters
    }

    fn last_data_unit(&self) -> u64 {
        self.total_clusters - 1
    }
}

impl BitmapFsMeta for NtfsMeta {
    #[inline]
    fn bitmap_offset(&self) -> u64 {
        self.lcn_to_offset(self.bitmap_lcn)
    }

    #[inline]
    fn bitmap_size(&self) -> u64 {
        self.bitmap_size_bytes
    }

    #[inline]
    fn bitmap_valid_bits(&self) -> u64 {
        self.total_units()
    }
}

/// Determines optimal cluster size based on volume size
fn determine_cluster_size(size_bytes: u64) -> u32 {
    const GB: u64 = 1024 * 1024 * 1024;
    const DEFAULT_CLUSTER_LIMIT: u64 = 16 * GB;

    // Windows and mkfs.ntfs default to 4 KiB clusters for ordinary NTFS
    // volumes. Smaller clusters make the fixed 16-sector $Boot file collide
    // with early system extents unless every layout calculation is adjusted.
    match size_bytes {
        0..=DEFAULT_CLUSTER_LIMIT => NTFS_DEFAULT_CLUSTER_SIZE,
        _ if size_bytes <= 32 * GB => 2 * NTFS_DEFAULT_CLUSTER_SIZE, // 16 GB - 32 GB: 8 KB
        _ if size_bytes <= 64 * GB => 4 * NTFS_DEFAULT_CLUSTER_SIZE, // 32 GB - 64 GB: 16 KB
        _ if size_bytes <= 128 * GB => 8 * NTFS_DEFAULT_CLUSTER_SIZE, // 64 GB - 128 GB: 32 KB
        _ => 65536,                         // > 128 GB: 64 KB
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
    fn invalid_record_encoding_is_rejected_before_mft_read() {
        use zerocopy::IntoBytes;
        let meta = NtfsMeta::new(100 * 1024 * 1024, None).unwrap();
        for encoding in [0, -32, -128] {
            let mut boot = NtfsBootSector::new_from_meta(&meta);
            boot.clusters_per_mft_record = encoding;
            let mut bytes = [0; 512];
            bytes.copy_from_slice(boot.as_bytes());
            let mut io = rimio::MemRimIO::new(&mut bytes);
            assert!(matches!(
                NtfsMeta::from_io(&mut io),
                Err(FsError::Invalid("Invalid NTFS record size"))
            ));
        }
    }

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
        assert_eq!(small.bytes_per_cluster, NTFS_DEFAULT_CLUSTER_SIZE);

        // Medium volume
        let medium = NtfsMeta::new(4 * 1024 * 1024 * 1024, None).unwrap();
        assert_eq!(medium.bytes_per_cluster, 4096);

        // Large volume
        let large = NtfsMeta::new(200 * 1024 * 1024 * 1024, None).unwrap();
        assert!(large.bytes_per_cluster >= 32768);
    }

    #[test]
    fn test_system_extents_do_not_overlap_boot_file() {
        let meta = NtfsMeta::new_custom(
            128 * 1024 * 1024,
            Some("TEST"),
            None,
            NTFS_SECTOR_SIZE,
            512,
            NTFS_MFT_RECORD_SIZE,
            NTFS_INDEX_RECORD_SIZE,
            0,
            UpcaseFlavor::Legacy,
        )
        .unwrap();

        let boot_clusters =
            (16 * meta.bytes_per_sector as u64).div_ceil(meta.bytes_per_cluster as u64);
        let mirr_clusters =
            (4 * meta.mft_record_size as u64).div_ceil(meta.bytes_per_cluster as u64);

        assert!(meta.mft_mirr_lcn >= boot_clusters);
        assert!(meta.logfile_lcn >= meta.mft_mirr_lcn + mirr_clusters);
    }
}
