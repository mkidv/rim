// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{BlockIO, BlockIOExt};
use zerocopy::IntoBytes;

pub use crate::core::formatter::*;

use crate::fs::fat32::{constant::*, meta::*, types::*};

/// Fat32Formatter:
/// - Fast + valid formatter.
/// - Prepares VBR, FSINFO, FAT initial entries, and Root Directory.
/// - Does not pre-allocate full FAT chains (injector is responsible for filling remaining data).
/// - Suitable for image generators (rimgen), bootable FS, and validated FAT32 structures.
pub struct Fat32Formatter<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Fat32Meta,
}

impl<'a, IO: BlockIO + ?Sized> Fat32Formatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Fat32Meta) -> Self {
        Self { io, meta }
    }

    fn write_vbr(&mut self) -> FsFormatterResult {
        let vbr = Fat32Vbr::from_meta(self.meta);

        // Write VBR to sector 0
        let offset = FAT_VBR_SECTOR * self.meta.sector_size as u64;
        self.io.write_at(offset, vbr.as_bytes())?;

        // Write backup VBR
        let backup_offset = FAT_VBR_BACKUP_SECTOR * self.meta.sector_size as u64;
        self.io.write_at(backup_offset, vbr.as_bytes())?;

        Ok(())
    }

    fn write_fsinfo(&mut self) -> FsFormatterResult {
        let fsinfo = Fat32FsInfo::from_meta(self.meta);

        // Write main FSINFO
        let offset = FAT_FSINFO_SECTOR * self.meta.sector_size as u64;
        self.io.write_at(offset, fsinfo.as_bytes())?;

        // Write backup FSINFO
        let backup_offset = FAT_FSINFO_BACKUP_SECTOR * self.meta.sector_size as u64;
        self.io.write_at(backup_offset, fsinfo.as_bytes())?;

        Ok(())
    }

    fn write_fat_region(&mut self) -> FsFormatterResult {
        for fat_index in 0..self.meta.num_fats {
            let mut buf = [0u8; FAT_RESERVED_ENTRIES.len() + FAT_ENTRY_SIZE];
            buf[..FAT_RESERVED_ENTRIES.len()].copy_from_slice(FAT_RESERVED_ENTRIES);

            let offset = self.meta.fat_offset
                + (fat_index as u64 * self.meta.fat_size as u64 * self.meta.sector_size as u64);

            self.io.write_at(offset, &buf)?;

            let written = buf.len() as u64;
            let total_fat_bytes = self.meta.fat_size as u64 * self.meta.sector_size as u64;
            let remaining = total_fat_bytes.saturating_sub(written);
            self.io.zero_fill(offset + written, remaining as usize)?;
        }

        Ok(())
    }

    fn write_root_dir_cluster(&mut self) -> FsFormatterResult {
        let mut buf = Vec::with_capacity(self.meta.unit_size());

        Fat32Entries::volume_label(self.meta.volume_label).to_raw_buffer(&mut buf);
        Fat32Entries::dot(self.meta.root_unit()).to_raw_buffer(&mut buf);
        Fat32Entries::dotdot(self.meta.root_unit()).to_raw_buffer(&mut buf);

        let offset = self.meta.unit_offset(self.meta.root_unit());
        self.io.write_at(offset, &buf)?;

        let written = buf.len();
        let remaining = self.meta.unit_size().saturating_sub(written);
        self.io.zero_fill(offset + (written as u64), remaining)?;

        let clusters = buf.len().div_ceil(self.meta.unit_size());
        self.write_fat_chain(self.meta.root_unit(), clusters as u32)?;

        Ok(())
    }

    fn zero_cluster_heap(&mut self) -> FsFormatterResult {
        // The cluster heap starts at ROOT_CLUSTER (already written) → start at ROOT_CLUSTER + 1
        // cluster_count = number of usable clusters (excluding reserved ones)
        for cluster in self.meta.first_data_unit()..self.meta.last_data_unit() {
            let offset = self.meta.unit_offset(cluster);
            self.io.zero_fill(offset, self.meta.unit_size())?;
        }
        Ok(())
    }

    fn write_fat_chain(&mut self, start_cluster: u32, cluster_count: u32) -> FsFormatterResult {
        for i in 0..cluster_count {
            let entry = if i + 1 < cluster_count {
                start_cluster + i + 1
            } else {
                FAT_EOC
            };
            for fat_index in 0..self.meta.num_fats {
                let offset = self.meta.fat_entry_offset(start_cluster + i, fat_index);
                self.io.write_at(offset, &entry.to_le_bytes())?;
            }
        }
        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsFormatter for Fat32Formatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult {
        self.write_vbr()?;
        self.write_fsinfo()?;
        self.write_fat_region()?;
        self.write_root_dir_cluster()?;
        if full_format {
            self.zero_cluster_heap()?;
        }
        self.io.flush()?;
        Ok(())
    }
}
