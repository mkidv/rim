// SPDX-License-Identifier: MIT

pub use crate::core::meta::*;

use crate::{core::cursor::ClusterMeta, fs::fat32::constant::*};

#[derive(Debug, Clone)]
pub struct Fat32Meta {
    pub volume_id: u32,
    pub volume_label: [u8; 11],

    pub(crate) bytes_per_sector: u16,
    pub(crate) sectors_per_cluster: u8,
    pub bytes_per_cluster: u32,

    pub volume_size_bytes: u64,
    pub(crate) volume_size_sectors: u64,

    pub(crate) num_fats: u8,
    pub fat_offset_bytes: u64,
    pub fat_size_sectors: u32,

    pub cluster_heap_offset: u64,
    pub cluster_count: u32,

    root_cluster: u32,
}

impl Fat32Meta {
    pub fn new(size_bytes: u64, volume_label: Option<&str>) -> Self {
        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            FAT_NUM_FATS,
            FAT_SECTOR_SIZE,
            FAT_CLUSTER_SIZE,
            DEFAULT_FAT_RESERVED_SECTORS as u32,
        )
    }

    pub fn new_custom(
        volume_size_bytes: u64,
        volume_label: Option<&str>,
        volume_id: u32,
        num_fats: u8,
        bytes_per_sector: u16,
        bytes_per_cluster: u32,
        reserved_sectors: u32,
    ) -> Self {
        let sectors_per_cluster = bytes_per_cluster
            .checked_div(bytes_per_sector as u32)
            .expect("cluster_size must be a multiple of sector_size")
            as u8;

        let mut volume_label_safe = [b' '; 11];
        if let Some(label) = &volume_label {
            for (i, b) in label.bytes().take(11).enumerate() {
                volume_label_safe[i] = b.to_ascii_uppercase();
            }
        }

        let volume_size_sectors = volume_size_bytes / bytes_per_sector as u64;

        let (fat_size_sectors, cluster_count) = converge_fat_layout(
            bytes_per_sector as u32,
            volume_size_sectors,
            reserved_sectors,
            FAT_ENTRY_SIZE as u32,
            FAT_FIRST_CLUSTER,
            num_fats,
            sectors_per_cluster as u32,
        );

        let fat_offset_bytes = reserved_sectors as u64 * bytes_per_sector as u64;
        let cluster_heap_offset =
            fat_offset_bytes + fat_size_sectors as u64 * num_fats as u64 * bytes_per_sector as u64;

        Self {
            volume_id,
            volume_label: volume_label_safe,
            bytes_per_sector,
            sectors_per_cluster,
            bytes_per_cluster,
            volume_size_bytes,
            volume_size_sectors,
            num_fats,
            fat_offset_bytes,
            fat_size_sectors,
            cluster_heap_offset,
            cluster_count,
            root_cluster: FAT_ROOT_CLUSTER,
        }
    }

    #[inline]
    pub fn root_clusters(&self) -> u32 {
        1
    }

    #[inline]
    pub fn system_used_clusters(&self) -> u32 {
        self.root_clusters()
    }

    pub fn percent_in_use(&self) -> u8 {
        if self.cluster_count == 0 {
            return 0;
        }
        let p = (self.system_used_clusters() as u64 * 100) / (self.cluster_count as u64);
        p.min(100) as u8
    }
}

impl FsMeta<u32> for Fat32Meta {
    fn unit_size(&self) -> usize {
        self.bytes_per_cluster as usize
    }

    fn root_unit(&self) -> u32 {
        self.root_cluster
    }

    fn total_units(&self) -> usize {
        self.cluster_count as usize
    }

    fn size_bytes(&self) -> u64 {
        self.volume_size_bytes
    }

    fn unit_offset(&self, cluster: u32) -> u64 {
        self.cluster_heap_offset + ((cluster - FAT_FIRST_CLUSTER) as u64 * self.unit_size() as u64)
    }

    fn first_data_unit(&self) -> u32 {
        self.root_cluster + self.root_clusters()
    }

    fn last_data_unit(&self) -> u32 {
        FAT_FIRST_CLUSTER + self.cluster_count - 1
    }
}

impl ClusterMeta for Fat32Meta {
    const EOC: u32 = FAT_EOC;
    const FIRST_CLUSTER: u32 = FAT_FIRST_CLUSTER;
    const ENTRY_SIZE: usize = FAT_ENTRY_SIZE;
    const ENTRY_MASK: u32 = FAT_MASK; // FAT32 utilise 28 bits

    fn fat_entry_offset(&self, cluster: u32, fat_index: u8) -> u64 {
        self.fat_offset_bytes
            + fat_index as u64 * self.fat_size_sectors as u64 * self.bytes_per_sector as u64
            + cluster as u64 * FAT_ENTRY_SIZE as u64
    }
}

/// Computes the FAT size and cluster count for a given FAT configuration.
///
/// This function performs convergence to determine the optimal FAT size (`fat_size`)
/// and the number of clusters (`cluster_count`) based on the FAT file system parameters.
///
/// # Arguments
/// - `sector_size`: Size of a sector in bytes (e.g., 512)
/// - `total_sectors`: Total number of sectors on the volume
/// - `reserved_sectors`: Number of reserved sectors (before the FAT area)
/// - `entry_size`: Size of a FAT entry (in bytes, e.g., 4 for FAT32)
/// - `min_entries`: Minimum number of FAT entries (often 2 for FAT12/16/32)
/// - `fat_count`: Number of FAT copies (usually 2)
/// - `sectors_per_cluster`: Number of sectors per cluster
///
/// # Returns
/// Tuple `(fat_size, cluster_count)`
/// - `fat_size`: FAT size in sectors
/// - `cluster_count`: Number of data clusters
pub fn converge_fat_layout(
    sector_size: u32,
    total_sectors: u64,
    reserved_sectors: u32,
    entry_size: u32,
    min_entries: u32,
    num_fats: u8,
    sectors_per_cluster: u32,
) -> (u32, u32) {
    assert!(sector_size != 0 && sectors_per_cluster != 0);
    let spc = sectors_per_cluster as u64;
    let reserved = reserved_sectors as u64;

    let mut cluster_count = 0u32;
    let mut fat_size = 0u32;

    for _ in 0..32 {
        let entries = cluster_count + min_entries;
        let fat_size_new = (entries * entry_size).div_ceil(sector_size);
        let fat_area = fat_size as u64 * num_fats as u64;
        let data_sectors = total_sectors
            .saturating_sub(reserved)
            .saturating_sub(fat_area);
        let cluster_count_new = (data_sectors / spc) as u32;

        if cluster_count_new == cluster_count && fat_size_new == fat_size {
            break;
        }

        cluster_count = cluster_count_new;
        fat_size = fat_size_new;
    }

    (fat_size, cluster_count)
}
