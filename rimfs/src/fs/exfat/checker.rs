// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;
use zerocopy::FromBytes;

use crate::core::{checker::*, meta::*};
use crate::fs::exfat::{constant::*, meta::*, parser::*, types::*, utils};

pub struct ExFatChecker<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
}

impl<'a, IO: BlockIO + ?Sized> ExFatChecker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        Self { io, meta }
    }

    pub fn check_vbr(&mut self) -> FsCheckerResult {
        let vbr: ExFatBootSector = self.io.read_struct(EXFAT_VBR_SECTOR)?;

        // Check signature
        if vbr.signature != EXFAT_SIGNATURE {
            return Err(FsCheckerError::Invalid("VBR : Missing 0x55AA signature"));
        }

        // Check FS type label
        if &vbr.fs_name != EXFAT_FS_NAME {
            return Err(FsCheckerError::Invalid("VBR : Invalid FS name label"));
        }

        Ok(())
    }

    pub fn check_root_dir(&mut self) -> FsCheckerResult {
        // let offset = self.meta.unit_offset(self.meta.root_cluster);
        // let mut buf = vec![0u8; self.meta.unit_size()];
        // self.io
        //     .read_at(offset, &mut buf)
        //     .map_err(FsCheckerError::IO)?;

        // let mut chunks = buf.chunks_exact(32);
        // let entry_label = chunks
        //     .next()
        //     .ok_or(FsCheckerError::Invalid("Missing Volume Label entry"))?;
        // let entry_guid = chunks
        //     .next()
        //     .ok_or(FsCheckerError::Invalid("Missing Guid entry"))?;
        // let entry_upcase = chunks
        //     .next()
        //     .ok_or(FsCheckerError::Invalid("Missing Upcase entry"))?;
        // let entry_bitmap = chunks
        //     .next()
        //     .ok_or(FsCheckerError::Invalid("Missing Bitmap entry"))?;

        // let entry_eod = chunks
        //     .next()
        //     .ok_or(FsCheckerError::Invalid("Missing EOD entry"))?;

        // // Check volume label
        // let label = ExFatVolumeLabelEntry::read_from_bytes(entry_label)
        //     .map_err(|_| FsCheckerError::Invalid("Invalid Volume Label Entry"))?;
        // if label.entry_type != EXFAT_ENTRY_VOLUME_LABEL {
        //     return Err(FsCheckerError::Invalid("Volume Label entry type incorrect"));
        // }
        // if label.character_count == 0 || label.character_count > 11 {
        //     return Err(FsCheckerError::Invalid("Invalid volume label length"));
        // }

        // // Check upcase table
        // let upcase = ExFatUpcaseEntry::read_from_bytes(entry_upcase)
        //     .map_err(|_| FsCheckerError::Invalid("Invalid Upcase Entry"))?;
        // if upcase.entry_type != EXFAT_ENTRY_UPCASE_TABLE {
        //     return Err(FsCheckerError::Invalid("Upcase entry type incorrect"));
        // }
        // if upcase.first_cluster != self.meta.upcase_cluster {
        //     return Err(FsCheckerError::Invalid("Upcase cluster mismatch"));
        // }

        // // Check bitmap entry
        // let bitmap = ExFatBitmapEntry::read_from_bytes(entry_bitmap)
        //     .map_err(|_| FsCheckerError::Invalid("Invalid Bitmap Entry"))?;
        // if bitmap.entry_type != EXFAT_ENTRY_BITMAP {
        //     return Err(FsCheckerError::Invalid("Bitmap entry type incorrect"));
        // }
        // if bitmap.first_cluster != self.meta.bitmap_cluster {
        //     return Err(FsCheckerError::Invalid("Bitmap cluster mismatch"));
        // }

        // // Check EOD
        // if entry_eod[0] != EXFAT_ENTRY_END_OF_DIR {
        //     return Err(FsCheckerError::Invalid(
        //         "Missing EOD marker after volume label",
        //     ));
        // }

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

            while (2..EXFAT_EOC).contains(&current) {
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

    pub fn check_bitmap_fat_consistency(&mut self) -> FsCheckerResult {
        let fat_size_bytes = (self.meta.fat_size * self.meta.sector_size as u32) as usize;
        let mut fat = vec![0u8; fat_size_bytes];
        self.io
            .read_at(self.meta.fat_offset, &mut fat)
            .map_err(FsCheckerError::IO)?;

        let mut bitmap = vec![0u8; self.meta.unit_size()];
        self.io
            .read_at(self.meta.unit_offset(self.meta.bitmap_cluster), &mut bitmap)
            .map_err(FsCheckerError::IO)?;

        let cluster_start = EXFAT_FIRST_CLUSTER;
        let cluster_end = cluster_start + self.meta.cluster_count;

        for cluster in cluster_start..cluster_end {
            let fat_index = (cluster * 4) as usize;
            if fat_index + 4 > fat.len() {
                return Err(FsCheckerError::Invalid("FAT index out of bounds"));
            }

            let fat_entry = u32::from_le_bytes(
                fat[fat_index..fat_index + 4]
                    .try_into()
                    .expect("slice length already verified"),
            );

            let (byte_index, bit_mask) = self.meta.bitmap_entry_offset(cluster);

            if byte_index >= bitmap.len() {
                return Err(FsCheckerError::Invalid("Bitmap index out of bounds"));
            }

            let bitmap_set = (bitmap[byte_index] & bit_mask) != 0;
            let fat_used = match fat_entry {
                0x00000000 => false,             // libre
                0x00000001 => true,              // réservé
                0x00000002..=0xFFFFFFF6 => true, // chaînage
                0xFFFFFFF7 => false,             // mauvais secteur
                0xFFFFFFF8..=0xFFFFFFFF => true, // réservé / EOC
                _ => false,
            };

            if bitmap_set != fat_used {
                eprintln!(
                    "❌ Cluster {cluster}: FAT = {fat_entry:#010X}, bitmap = {bitmap_set}, expected = {fat_used}"
                );
                return Err(FsCheckerError::Invalid("Cluster bitmap and FAT mismatch"));
            }
        }

        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsChecker for ExFatChecker<'a, IO> {
    fn check_all(&mut self) -> FsCheckerResult {
        self.check_vbr()?;
        self.check_root_dir()?;
        self.check_fat_chains()?;
        self.check_bitmap_fat_consistency()?;
        Ok(())
    }
}
