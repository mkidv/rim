// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::BlockIO;
use zerocopy::FromBytes;

use crate::core::{checker::*, meta::*};
use crate::fs::fat32::{constant::*, meta::*, parser::*, types::*};

pub struct Fat32Checker<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Fat32Meta,
}

impl<'a, IO: BlockIO + ?Sized> Fat32Checker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Fat32Meta) -> Self {
        Self { io, meta }
    }

    fn read_struct<T: FromBytes>(&mut self, sector: u64) -> FsCheckerResult<T> {
        let mut buf = [0u8; FAT_MAX_SECTOR_SIZE];
        let size = self.meta.sector_size as usize;
        assert!(
            size <= FAT_MAX_SECTOR_SIZE,
            "Sector size exceeds max buffer size"
        );
        self.io
            .read_at(sector * self.meta.sector_size as u64, &mut buf[..size])
            .map_err(FsCheckerError::IO)?;
        T::read_from_bytes(&buf[..size]).map_err(|_| FsCheckerError::Invalid("Invalid structure"))
    }

    pub fn check_vbr(&mut self) -> FsCheckerResult {
        let vbr: Fat32Vbr = self.read_struct(FAT_VBR_SECTOR)?;

        // Check signature
        if vbr.signature != FAT_SIGNATURE {
            return Err(FsCheckerError::Invalid("VBR : Missing 0x55AA signature"));
        }

        // Check FAT32 type label
        if &vbr.fs_type != FAT_FS_TYPE {
            return Err(FsCheckerError::Invalid("VBR : Invalid type label"));
        }

        Ok(())
    }

    pub fn check_fsinfo(&mut self) -> FsCheckerResult {
        let fsinfo: Fat32FsInfo = self.read_struct(FAT_FSINFO_SECTOR)?;

        // Check lead signature
        if &fsinfo.lead_signature != FAT_FSINFO_LEAD_SIGNATURE {
            return Err(FsCheckerError::Invalid(
                "FSINFO : Missing lead signature RRaA",
            ));
        }

        // Check structure signature
        if &fsinfo.struct_signature != FAT_FSINFO_STRUCT_SIGNATURE {
            return Err(FsCheckerError::Invalid(
                "FSINFO : Missing structure signature rrAa",
            ));
        }

        // Check signature
        if fsinfo.trail_signature != FAT_FSINFO_TRAIL_SIGNATURE {
            return Err(FsCheckerError::Invalid("FSINFO : Missing 0x55AA signature"));
        }

        Ok(())
    }

    pub fn check_root_dir(&mut self) -> FsCheckerResult {
        // Check FAT[2] == EOC
        let fat_entry_offset = self.meta.fat_entry_offset(self.meta.root_unit(), 0);
        let mut fat_entry_buf = [0u8; 4];
        self.io
            .read_at(fat_entry_offset, &mut fat_entry_buf)
            .map_err(FsCheckerError::IO)?;

        let fat_entry = u32::from_le_bytes(fat_entry_buf) & 0x0FFFFFFF;

        if fat_entry < FAT_EOC {
            return Err(FsCheckerError::Invalid("FAT[ROOT_CLUSTER] is not EOC"));
        }

        // Then check root dir content
        let root_offset = self.meta.unit_offset(self.meta.root_unit());

        let mut buf = [0u8; FAT_CLUSTER_SIZE as usize];
        self.io
            .read_at(root_offset, &mut buf)
            .map_err(FsCheckerError::IO)?;

        Ok(())
    }

    fn check_fat_chains(&mut self) -> FsCheckerResult {
        let first_cluster = self.meta.first_data_unit();
        let last_cluster = self.meta.last_data_unit();
        let cluster_span = (last_cluster - first_cluster) as usize;

        let bitmap_size = cluster_span.div_ceil(8);
        let mut visited_bitmap = vec![0u8; bitmap_size];

        let mark_visited = |bitmap: &mut [u8], cluster: u32| {
            let idx = (cluster - first_cluster) as usize;
            bitmap[idx / 8] |= 1 << (idx % 8);
        };

        let is_visited = |bitmap: &[u8], cluster: u32| -> bool {
            let idx = (cluster - first_cluster) as usize;
            (bitmap[idx / 8] & (1 << (idx % 8))) != 0
        };

        for cluster in first_cluster..last_cluster {
            if is_visited(&visited_bitmap, cluster) {
                continue;
            }

            let mut current = cluster;
            let mut chain_len = 0;

            while (2..FAT_EOC).contains(&current) {
                if current < first_cluster || current >= last_cluster {
                    return Err(FsCheckerError::Invalid("Cluster out of range in FAT chain"));
                }

                if is_visited(&visited_bitmap, current) {
                    return Err(FsCheckerError::Invalid("Loop detected in FAT chain"));
                }

                mark_visited(&mut visited_bitmap, current);

                current = read_fat_entry(self.io, self.meta, current)?;
                chain_len += 1;

                if chain_len > self.meta.cluster_count as usize {
                    return Err(FsCheckerError::Invalid("Invalid FAT chain length"));
                }
            }
        }

        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsChecker for Fat32Checker<'a, IO> {
    fn check_all(&mut self) -> FsCheckerResult {
        self.check_vbr()?;
        self.check_fsinfo()?;
        self.check_root_dir()?;
        self.check_fat_chains()?;
        Ok(())
    }
}
