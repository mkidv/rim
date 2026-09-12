// SPDX-License-Identifier: MIT

//! FAT volume metadata and geometry convergence calculation.

use alloc::string::{String, ToString};
use rimio::prelude::*;

use crate::core::errors::{FsError, FsResult};
use crate::core::fat::FatFsMeta;
pub use crate::core::meta::*;

use crate::{constant::*, types::FatVbr};

#[derive(Debug, Clone)]
pub struct FatMeta {
    pub volume_id: u32,
    pub volume_label: [u8; 11],

    pub(crate) bytes_per_sector: u16,
    pub(crate) sectors_per_cluster: u32,
    pub bytes_per_cluster: u32,

    pub volume_size_bytes: u64,
    pub(crate) volume_size_sectors: u64,

    pub(crate) num_fats: u8,
    pub root_entry_count: u16,
    pub fat_offset_bytes: u64,
    pub fat_size_sectors: u32,

    pub cluster_heap_offset_bytes: u64,
    pub cluster_count: u32,

    pub bits: u8,
    pub root_cluster: u32,
    pub use_integrity: bool,
    pub use_fat_integrity: bool, // RimFAT: Global FAT table integrity
    pub is_dirty: bool,          // RimFAT: Volume was not cleanly unmounted
}

impl FatMeta {
    pub fn new_fat32(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        // Microsoft FAT32 specification:
        // A FAT volume is FAT32 if and only if cluster_count >= 65525.
        // For volumes < 260 MB (e.g. 32MB - 64MB ESP partitions), cluster size must be 512 bytes
        // (1 sector/cluster) to guarantee cluster_count >= 65525 and pass all fsck.fat checks.
        let cluster_size = if size_bytes < 260 * 1024 * 1024 {
            512
        } else if size_bytes <= 8 * 1024 * 1024 * 1024 {
            4096
        } else if size_bytes <= 16 * 1024 * 1024 * 1024 {
            8192
        } else if size_bytes <= 32 * 1024 * 1024 * 1024 {
            16384
        } else {
            32768
        };

        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            FAT_NUM_FATS,
            FAT_SECTOR_SIZE,
            cluster_size,
            DEFAULT_FAT_RESERVED_SECTORS as u32,
            0, // root_entry_count (FAT32)
            32,
        )
    }

    pub fn new_fat16(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let max_clusters = FAT16_MAX_CLUSTERS;
        let mut cluster_size = 4096;

        while (size_bytes / cluster_size as u64) > max_clusters as u64 {
            cluster_size *= 2;
            if cluster_size > 65536 {
                return Err(FsError::Invalid("Volume too large for FAT16"));
            }
        }

        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            FAT_NUM_FATS,
            FAT_SECTOR_SIZE,
            cluster_size,
            DEFAULT_FAT_RESERVED_SECTORS as u32,
            512, // root_entry_count (Standard FAT16)
            16,
        )
    }

    pub fn new_fat12(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let max_clusters = FAT12_MAX_CLUSTERS; // Safe FAT12 limit
        let mut cluster_size = 512;

        while (size_bytes / cluster_size as u64) > max_clusters as u64 {
            cluster_size *= 2;
            if cluster_size > 65536 {
                return Err(FsError::Invalid("Volume too large for FAT12"));
            }
        }

        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            FAT_NUM_FATS,
            FAT_SECTOR_SIZE,
            cluster_size,
            2,   // Standard FAT12 usually has 1 or 2 reserved sectors
            224, // root_entry_count (Standard FAT12)
            12,
        )
    }

    pub fn new_fat8(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        // FAT8 for extremely small legacy/toy volumes
        let max_clusters = FAT8_MAX_CLUSTERS;
        let mut cluster_size = 256;

        while (size_bytes / cluster_size as u64) > max_clusters as u64 {
            cluster_size *= 2;
            if cluster_size > 65536 {
                // FAT8 is likely for tiny volumes, so hitting 64KB cluster is absurd but handled.
                // Actually FAT8 entries are 8-bit. Max cluster is 255.
                return Err(FsError::Invalid("Volume too large for FAT8"));
            }
        }

        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            1,            // usually 1 FAT
            128,          // Small sectors sometimes
            cluster_size, // Adapted
            1,            // Minimal reserved
            32,           // Small root dir
            8,
        )
    }

    pub fn new_rimfat(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        let mut meta = Self::new_fat32(size_bytes, volume_label)?;
        meta.use_integrity = true;
        meta.use_fat_integrity = true;
        Ok(meta)
    }

    pub fn new_fat64(size_bytes: u64, volume_label: Option<&str>) -> FsResult<Self> {
        // FAT64: Use FAT32 layout but with 64-bit entries
        Self::new_custom(
            size_bytes,
            volume_label,
            generate_volume_id_32(),
            FAT_NUM_FATS,
            FAT_SECTOR_SIZE,
            FAT_CLUSTER_SIZE,
            DEFAULT_FAT_RESERVED_SECTORS as u32,
            0,
            64,
        )
    }

    pub fn from_io<IO: rimio::RimRead + ?Sized>(io: &mut IO) -> FsResult<Self> {
        let vbr: FatVbr = io.read_struct(0)?;

        if vbr.signature != FAT_SIGNATURE {
            return Err(FsError::Invalid("Invalid FAT VBR signature"));
        }

        let jmp = vbr.bpb.jump_boot;
        if !((jmp[0] == 0xEB && jmp[2] == 0x90) || jmp[0] == 0xE9) {
            return Err(FsError::Invalid("Invalid FAT boot jump instruction"));
        }

        let bytes_per_sector = vbr.bpb.bytes_per_sector.get();
        if !matches!(bytes_per_sector, 512 | 1024 | 2048 | 4096) {
            return Err(FsError::Invalid("Invalid FAT bytes_per_sector"));
        }
        let sectors_per_cluster = vbr.bpb.sectors_per_cluster as u32;
        if !sectors_per_cluster.is_power_of_two() || sectors_per_cluster > 128 {
            return Err(FsError::Invalid("Invalid FAT sectors_per_cluster"));
        }
        let reserved_sectors = vbr.bpb.reserved_sectors.get() as u32;
        if reserved_sectors == 0 {
            return Err(FsError::Invalid("Invalid FAT reserved_sectors"));
        }
        let num_fats = vbr.bpb.num_fats;
        if num_fats == 0 || num_fats > 4 {
            return Err(FsError::Invalid("Invalid FAT num_fats"));
        }
        let root_entry_count = vbr.bpb.root_entry_count.get();

        let total_sectors = if vbr.bpb.total_sectors_16.get() != 0 {
            vbr.bpb.total_sectors_16.get() as u64
        } else {
            vbr.bpb.total_sectors_32.get() as u64
        };
        if total_sectors == 0 {
            return Err(FsError::Invalid("Invalid FAT total_sectors"));
        }

        let fat_size_sectors = vbr.fat_size_sectors();

        let root_dir_sectors = (root_entry_count as u32 * 32).div_ceil(bytes_per_sector as u32);
        let fat_area_sectors = fat_size_sectors as u64 * num_fats as u64;
        let data_sectors = total_sectors
            .saturating_sub(reserved_sectors as u64)
            .saturating_sub(fat_area_sectors)
            .saturating_sub(root_dir_sectors as u64);

        let cluster_count = (data_sectors / sectors_per_cluster as u64) as u32;
        if cluster_count == 0 {
            return Err(FsError::Invalid("Invalid FAT: zero data clusters"));
        }

        // Microsoft FAT bit depth formula
        // Note: We prioritize the structural indicator (fat_size_16 == 0 => FAT32)
        // to support "Small FAT32" volumes often used in testing or embedded.
        let bits = if vbr.is_fat32() {
            // For now we default to 32, but in the future we might store the bits in an extended field.
            // If it's a known RimFAT extension, we might eventually find the bit depth here.
            32
        } else if cluster_count < 4085 {
            12
        } else if cluster_count < 65525 {
            16
        } else {
            32
        };

        let fat_offset_bytes = reserved_sectors as u64 * bytes_per_sector as u64;
        let cluster_heap_offset_bytes = fat_offset_bytes
            + (fat_area_sectors * bytes_per_sector as u64)
            + (root_dir_sectors as u64 * bytes_per_sector as u64);

        let (volume_id, volume_label, root_cluster) = if vbr.is_fat32() {
            let e = vbr.f32();
            if e.root_cluster.get() < FAT_FIRST_CLUSTER {
                return Err(FsError::Invalid("Invalid FAT32 root_cluster"));
            }
            (e.volume_id.get(), e.volume_label, e.root_cluster.get())
        } else {
            let e = vbr.f16();
            (e.volume_id.get(), e.volume_label, FAT_ROOT_CLUSTER)
        };

        let mut meta = Self {
            volume_id,
            volume_label,
            bytes_per_sector,
            sectors_per_cluster,
            bytes_per_cluster: sectors_per_cluster * bytes_per_sector as u32,
            volume_size_bytes: total_sectors * bytes_per_sector as u64,
            volume_size_sectors: total_sectors,
            num_fats,
            root_entry_count,
            fat_offset_bytes,
            fat_size_sectors,
            cluster_heap_offset_bytes,
            cluster_count,
            bits,
            root_cluster,
            use_integrity: false,
            use_fat_integrity: false,
            is_dirty: false,
        };

        // RimFAT Specifics: Detect if volume was formatted as RimFAT
        if vbr.bpb.oem_name == *FAT_OEM_NAME {
            meta.use_integrity = true;
            meta.use_fat_integrity = true;

            // 1. Check Transaction Dirty Bit
            let mut trans_buf = [0u8; 512];
            io.read_at(
                FAT_TRANSACTION_SECTOR * bytes_per_sector as u64,
                &mut trans_buf,
            )?;
            if &trans_buf[0..5] == b"DIRTY" {
                meta.is_dirty = true;
            }

            // 2. Verify FAT Checksum (from FSINFO)
            let fsinfo_off = FAT_FSINFO_SECTOR * bytes_per_sector as u64;
            let fsinfo: crate::types::FatFsInfo = io.read_struct(fsinfo_off)?;
            let expected_fat_crc = fsinfo.fat_checksum.get();

            if expected_fat_crc != 0 {
                let mut fat_buf =
                    vec![0u8; (meta.fat_size_sectors * bytes_per_sector as u32) as usize];
                io.read_at(meta.fat_offset_bytes, &mut fat_buf)?;
                let actual_fat_crc = crate::core::utils::checksum_utils::crc32(&fat_buf);
                if expected_fat_crc != actual_fat_crc {
                    return Err(FsError::Invalid("RimFAT: Global FAT integrity failure"));
                }
            }
        }

        Ok(meta)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_custom(
        volume_size_bytes: u64,
        volume_label: Option<&str>,
        volume_id: u32,
        num_fats: u8,
        bytes_per_sector: u16,
        bytes_per_cluster: u32,
        reserved_sectors: u32,
        root_entry_count: u16,
        bits: u8,
    ) -> FsResult<Self> {
        let sectors_per_cluster = bytes_per_cluster
            .checked_div(bytes_per_sector as u32)
            .ok_or(FsError::Invalid(
                "cluster_size must be a multiple of sector_size",
            ))?;

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
            root_entry_count,
            bits,
            Self::FIRST_CLUSTER, // Use trait-bound constant
            num_fats,
            sectors_per_cluster,
        );

        let fat_offset_bytes = reserved_sectors as u64 * bytes_per_sector as u64;
        let root_dir_sectors = (root_entry_count as u32 * 32).div_ceil(bytes_per_sector as u32);

        let cluster_heap_offset_bytes = fat_offset_bytes
            + (fat_size_sectors as u64 * num_fats as u64 * bytes_per_sector as u64)
            + (root_dir_sectors as u64 * bytes_per_sector as u64);

        Ok(Self {
            volume_id,
            volume_label: volume_label_safe,
            bytes_per_sector,
            sectors_per_cluster,
            bytes_per_cluster,
            volume_size_bytes,
            volume_size_sectors,
            num_fats,
            root_entry_count,
            fat_offset_bytes,
            fat_size_sectors,
            cluster_heap_offset_bytes,
            cluster_count,
            bits,
            root_cluster: FAT_ROOT_CLUSTER,
            use_integrity: false,
            use_fat_integrity: false,
            is_dirty: false,
        })
    }

    #[inline]
    pub fn root_clusters(&self) -> u32 {
        if self.root_entry_count > 0 {
            (self.root_entry_count as u32 * 32).div_ceil(self.bytes_per_cluster)
        } else {
            1
        }
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

    pub fn root_dir_size_bytes(&self) -> usize {
        if self.root_entry_count > 0 {
            self.root_entry_count as usize * 32
        } else {
            self.bytes_per_cluster as usize
        }
    }
}

impl FsMeta<u32> for FatMeta {
    fn unit_size(&self) -> u64 {
        self.bytes_per_cluster as u64
    }

    fn root_unit(&self) -> u32 {
        if self.root_entry_count > 0 {
            1 // Virtual root unit
        } else {
            self.root_cluster
        }
    }

    fn total_units(&self) -> u64 {
        self.cluster_count as u64
    }

    fn size_bytes(&self) -> u64 {
        self.volume_size_bytes
    }

    fn label(&self) -> String {
        String::from_utf8_lossy(&self.volume_label)
            .trim()
            .to_string()
    }

    fn unit_offset(&self, cluster: u32) -> u64 {
        if cluster == 1 && self.root_entry_count > 0 {
            // Fixed Root Directory offset
            self.fat_offset_bytes
                + (self.fat_size_sectors as u64
                    * self.num_fats as u64
                    * self.bytes_per_sector as u64)
        } else {
            self.cluster_heap_offset_bytes
                + (cluster.saturating_sub(Self::FIRST_CLUSTER) as u64 * self.unit_size())
        }
    }

    fn first_data_unit(&self) -> u32 {
        self.root_cluster + self.root_clusters()
    }

    fn last_data_unit(&self) -> u32 {
        Self::FIRST_CLUSTER + self.cluster_count - 1
    }
}

impl FatFsMeta for FatMeta {
    const FIRST_CLUSTER: u32 = FAT_FIRST_CLUSTER;

    fn bits_per_entry(&self) -> u32 {
        self.bits as u32
    }

    fn entry_mask(&self) -> u32 {
        match self.bits {
            8 => 0xFF,
            12 => 0xFFF,
            16 => 0xFFFF,
            32 => FAT_MASK,
            _ => (1 << self.bits) - 1,
        }
    }

    fn fat_table_offset(&self, fat_index: u8) -> u64 {
        self.fat_offset_bytes
            + fat_index as u64 * self.fat_size_sectors as u64 * self.bytes_per_sector as u64
    }

    fn fat_size_bytes(&self) -> u64 {
        self.fat_size_sectors as u64 * self.bytes_per_sector as u64
    }

    fn is_eoc(&self, cluster: u32) -> bool {
        if self.root_entry_count > 0 && cluster == 1 {
            return true;
        }
        cluster >= (self.entry_mask() & !0x7)
    }

    fn num_fats(&self) -> u8 {
        self.num_fats
    }
}

/// Computes optimal FAT size (in sectors) and data cluster count via iterative convergence.
#[allow(clippy::too_many_arguments)]
pub fn converge_fat_layout(
    sector_size: u32,
    total_sectors: u64,
    reserved_sectors: u32,
    root_entry_count: u16,
    bits_per_entry: u8,
    min_entries: u32,
    num_fats: u8,
    sectors_per_cluster: u32,
) -> (u32, u32) {
    assert!(sector_size != 0 && sectors_per_cluster != 0);
    let spc = sectors_per_cluster as u64;
    let reserved = reserved_sectors as u64;
    let root_dir_sectors = (root_entry_count as u32 * 32).div_ceil(sector_size) as u64;

    let mut cluster_count = 0u32;
    let mut fat_size = 0u32;

    for _ in 0..32 {
        let entries = cluster_count + min_entries;
        let fat_size_bits = entries as u64 * bits_per_entry as u64;
        let fat_size_bytes = fat_size_bits.div_ceil(8);
        let fat_size_new = fat_size_bytes.div_ceil(sector_size as u64) as u32;

        let fat_area = fat_size_new as u64 * num_fats as u64;
        let data_sectors = total_sectors
            .saturating_sub(reserved)
            .saturating_sub(fat_area)
            .saturating_sub(root_dir_sectors);
        let cluster_count_new = (data_sectors / spc) as u32;

        if cluster_count_new == cluster_count && fat_size_new == fat_size {
            break;
        }

        cluster_count = cluster_count_new;
        fat_size = fat_size_new;
    }

    (fat_size, cluster_count)
}
