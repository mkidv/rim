// SPDX-License-Identifier: MIT

pub use crate::core::meta::*;

use crate::fs::fat32::constant::*;

#[derive(Debug, Clone)]
pub struct Fat32Meta {
    pub(crate) sector_size: u16,
    pub volume_label: [u8; 11],
    pub volume_id: u32,
    pub size_bytes: u64,
    pub(crate) total_sectors: u32,
    pub(crate) sectors_per_cluster: u8,
    pub(crate) num_fats: u8,
    pub fat_offset: u64,
    pub fat_size: u32,
    pub cluster_heap_offset: u64,
    cluster_size: u32,
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
        size_bytes: u64,
        volume_label: Option<&str>,
        volume_id: u32,
        num_fats: u8,
        sector_size: u16,
        cluster_size: u32,
        reserved_sectors: u32,
    ) -> Self {
        let sectors_per_cluster = cluster_size
            .checked_div(sector_size as u32)
            .expect("cluster_size must be a multiple of sector_size")
            as u8;

        let mut volume_label_safe = [b' '; 11];
        if let Some(label) = &volume_label {
            for (i, b) in label.bytes().take(11).enumerate() {
                volume_label_safe[i] = b.to_ascii_uppercase();
            }
        }

        let total_sectors = (size_bytes / sector_size as u64) as u32;

        let (fat_size, cluster_count) = converge_fat_layout(
            sector_size as u32,
            total_sectors,
            reserved_sectors,
            FAT_ENTRY_SIZE as u32,
            FAT_RESERVED_ENTRIES.len() as u32,
            num_fats,
            sectors_per_cluster as u32,
        );

        let fat_offset = reserved_sectors as u64 * sector_size as u64;
        let cluster_heap_offset =
            fat_offset + fat_size as u64 * num_fats as u64 * sector_size as u64;

        Self {
            sector_size,
            volume_label: volume_label_safe,
            volume_id,
            size_bytes,
            total_sectors,
            sectors_per_cluster,
            num_fats,
            fat_offset,
            fat_size,
            cluster_heap_offset,
            cluster_size,
            cluster_count,
            root_cluster : FAT_ROOT_CLUSTER ,
        }
    }

    pub fn fat_entry_offset(&self, cluster: u32, fat_index: u8) -> u64 {
        self.fat_offset
            + fat_index as u64 * self.fat_size as u64 * self.sector_size as u64
            + cluster as u64 * FAT_ENTRY_SIZE as u64
    }
}

impl FsMeta<u32> for Fat32Meta {
    fn unit_size(&self) -> usize {
        self.cluster_size as usize
    }

    fn root_unit(&self) -> u32 {
        self.root_cluster
    }

    fn total_units(&self) -> usize {
        self.cluster_count as usize
    }

    fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    fn unit_offset(&self, cluster: u32) -> u64 {
        self.cluster_heap_offset + ((cluster - FAT_FIRST_CLUSTER) as u64 * self.unit_size() as u64)
    }

    fn first_data_unit(&self) -> u32 {
        self.root_unit() + FAT_PADDING + 1
    }

    fn last_data_unit(&self) -> u32 {
        self.first_data_unit() + self.total_units() as u32 - 1
    }
}
