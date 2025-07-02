// SPDX-License-Identifier: MIT

use rimio::{BlockIO, BlockIOStructExt, error::BlockIOError};
use zerocopy::FromBytes;

pub use crate::core::meta::*;

use crate::{
    core::FsResult,
    fs::exfat::{constant::*, types::*},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ExFatMeta {
    pub sector_size: u16,
    pub volume_label: [u16; 11],
    pub volume_id: u32,
    pub size_bytes: u64,
    pub total_sectors: u32,
    pub sectors_per_cluster: u8,
    pub num_fats: u8,
    pub fat_offset: u64,
    pub fat_size: u32,
    pub cluster_heap_offset: u64,
    pub cluster_size: u32,
    pub cluster_count: u32,
    pub bitmap_cluster: u32,
    pub upcase_cluster: u32,
    pub root_cluster: u32,
}

impl ExFatMeta {
    pub fn new(size_bytes: u64, volume_label: Option<&str>) -> Self {
        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            EXFAT_NUM_FATS,
            EXFAT_SECTOR_SIZE,
            EXFAT_CLUSTER_SIZE,
            EXFAT_RESERVED_SECTORS as u32,
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

        let mut volume_label_safe = [0u16; 11];
        if let Some(label) = volume_label {
            for (i, b) in label.encode_utf16().take(11).enumerate() {
                volume_label_safe[i] = b;
            }
        }

        let total_sectors = (size_bytes / sector_size as u64) as u32;

        let (fat_size, cluster_count) = converge_fat_layout(
            sector_size as u32,
            total_sectors,
            reserved_sectors,
            EXFAT_ENTRY_SIZE as u32,
            EXFAT_ROOT_CLUSTER + EXFAT_PADDING,
            EXFAT_NUM_FATS,
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
            root_cluster: EXFAT_ROOT_CLUSTER,
            bitmap_cluster: EXFAT_BITMAP_CLUSTER,
            upcase_cluster: EXFAT_UPCASE_CLUSTER,
        }
    }

    pub fn from_io<IO: BlockIO + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let vbr: ExFatBootSector = io.read_struct(EXFAT_VBR_SECTOR)?;

        let bytes_per_sector = 1u32 << vbr.bytes_per_sector_shift;
        let sectors_per_cluster = 1u32 << vbr.sectors_per_cluster_shift;
        let cluster_size = bytes_per_sector * sectors_per_cluster;
        let fat_offset = vbr.fat_offset as u64 * bytes_per_sector as u64;
        let cluster_heap_offset = vbr.cluster_heap_offset as u64 * bytes_per_sector as u64;

        let root_cluster = vbr.root_dir_cluster;
        let mut found_bitmap: Option<ExFatBitmapEntry> = None;
        let mut found_upcase: Option<ExFatUpcaseEntry> = None;
        let mut found_label: Option<ExFatVolumeLabelEntry> = None;

        let offset = cluster_heap_offset
            + ((root_cluster - EXFAT_FIRST_CLUSTER) as u64 * cluster_size as u64);
        let mut buf = vec![0u8; cluster_size as usize];
        io.read_at(offset, &mut buf)?;

        let entries = buf.chunks_exact(32);
        for entry in entries {
            let tag = entry[0];

            match tag {
                0x83 => {
                    // Volume Label
                    found_label = Some(
                        ExFatVolumeLabelEntry::read_from_bytes(entry)
                            .map_err(|_| BlockIOError::Error("volume_label_parse"))?,
                    );
                }
                0x81 => {
                    // Bitmap
                    found_bitmap = Some(
                        ExFatBitmapEntry::read_from_bytes(entry)
                            .map_err(|_| BlockIOError::Error("bitmap_parse"))?,
                    );
                }
                0x82 => {
                    // Upcase
                    found_upcase = Some(
                        ExFatUpcaseEntry::read_from_bytes(entry)
                            .map_err(|_| BlockIOError::Error("upcase_parse"))?,
                    );
                }
                0x00 => break, // End of Directory
                _ => {}
            }
        }

        let volume_label = found_label.map(|f| f.volume_label).unwrap_or([0u16; 11]);

        let bitmap_cluster = found_bitmap
            .ok_or(BlockIOError::Error("bitmap_cluster"))?
            .first_cluster;

        let upcase_cluster = found_upcase
            .ok_or(BlockIOError::Error("upcase_cluster"))?
            .first_cluster;

        Ok(Self {
            sector_size: bytes_per_sector as u16,
            volume_label,
            volume_id: vbr.volume_serial,
            size_bytes: vbr.volume_length * (bytes_per_sector as u64),
            total_sectors: vbr.volume_length as u32,
            sectors_per_cluster: sectors_per_cluster as u8,
            num_fats: vbr.number_of_fats,
            fat_offset,
            fat_size: vbr.fat_length,
            cluster_heap_offset,
            cluster_size,
            cluster_count: vbr.cluster_count,
            bitmap_cluster,
            upcase_cluster,
            root_cluster,
        })
    }

    pub fn fat_entry_offset(&self, cluster: u32) -> u64 {
        self.fat_offset + cluster as u64 * EXFAT_ENTRY_SIZE as u64
    }

    pub fn bitmap_bit_offset(&self, cluster: u32) -> usize {
        (cluster - EXFAT_FIRST_CLUSTER) as usize
    }

    pub fn bitmap_entry_offset(&self, cluster: u32) -> (usize, u8) {
        let bit = self.bitmap_bit_offset(cluster);
        let byte_index = bit / 8;
        let bit_mask = 1 << (bit % 8);
        (byte_index, bit_mask)
    }
}

impl FsMeta<u32> for ExFatMeta {
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
        self.cluster_heap_offset
            + ((cluster - EXFAT_FIRST_CLUSTER) as u64 * self.unit_size() as u64)
    }

    fn first_data_unit(&self) -> u32 {
        self.root_unit() + EXFAT_PADDING +1
    }

    fn last_data_unit(&self) -> u32 {
        self.first_data_unit() + self.total_units() as u32 - 1
    }
}
