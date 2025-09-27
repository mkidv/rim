// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{BlockIO, BlockIOExt};
use zerocopy::IntoBytes;

pub use crate::core::formatter::*;

use crate::{
    core::cursor::ClusterMeta,
    fs::fat32::{constant::*, meta::*, ops, types::*},
};

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
        let offset = FAT_VBR_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(offset, vbr.as_bytes())?;

        // Write backup VBR
        let backup_offset = FAT_VBR_BACKUP_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(backup_offset, vbr.as_bytes())?;

        Ok(())
    }

    fn write_fsinfo(&mut self) -> FsFormatterResult {
        let fsinfo = Fat32FsInfo::from_meta(self.meta);

        // Write main FSINFO
        let offset = FAT_FSINFO_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(offset, fsinfo.as_bytes())?;

        // Write backup FSINFO
        let backup_offset = FAT_FSINFO_BACKUP_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(backup_offset, fsinfo.as_bytes())?;

        Ok(())
    }

    fn write_fat_region(&mut self) -> FsFormatterResult {
        let total = self.meta.fat_size_sectors as usize * self.meta.bytes_per_sector as usize;

        let mut buf = vec![0u8; total];

        buf[0..4].copy_from_slice(&(0x0FFFFFF8 | (FAT_MEDIA_DESCRIPTOR as u32)).to_le_bytes());
        buf[4..8].copy_from_slice(&FAT_EOC.to_le_bytes());

        for fat_index in 0..self.meta.num_fats {
            let offset = self.meta.fat_entry_offset(0, fat_index);

            self.io.write_at(offset, &buf)?;

            let written = buf.len() as u64;
            let total_fat_bytes =
                self.meta.fat_size_sectors as u64 * self.meta.bytes_per_sector as u64;
            let remaining = total_fat_bytes.saturating_sub(written);
            self.io.zero_fill(offset + written, remaining as usize)?;
        }
        Ok(())
    }

    fn write_root_dir_cluster(&mut self) -> FsFormatterResult {
        let mut buf = Vec::with_capacity(self.meta.unit_size());

        Fat32Entries::volume_label(self.meta.volume_label).to_raw_buffer(&mut buf);
        Fat32EodEntry::new().to_raw_buffer(&mut buf);

        let offset = self.meta.unit_offset(self.meta.root_unit());
        self.io.write_at(offset, &buf)?;

        Ok(())
    }

    fn allocate_system_clusters(&mut self) -> FsFormatterResult {
        let root = self.meta.root_unit();
        ops::write_all_fat_chain(self.io, self.meta, &[root])?;
        Ok(())
    }

    fn zero_cluster_heap(&mut self) -> FsFormatterResult {
        let first = self.meta.first_data_unit();
        let last = self.meta.last_data_unit(); // inclusif
        if first > last {
            return Ok(());
        }

        let start = self.meta.unit_offset(first);
        let end = self.meta.unit_offset(last) + self.meta.unit_size() as u64; // inclure le dernier cluster
        let len = end.saturating_sub(start) as usize;

        self.io.zero_fill(start, len)?;
        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsFormatter for Fat32Formatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult {
        self.write_vbr()?;
        self.write_fsinfo()?;
        self.write_fat_region()?;
        self.write_root_dir_cluster()?;
        self.allocate_system_clusters()?;

        if full_format {
            self.zero_cluster_heap()?;
        }
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rimio::prelude::*;

    fn make_meta_32mb() -> Fat32Meta {
        const SIZE: u64 = 32 * 1024 * 1024;
        Fat32Meta::new(SIZE, Some("TESTFS"))
    }

    #[test]
    fn test_format_writes_vbr_fsinfo_fat_and_root() {
        let meta = make_meta_32mb();
        let mut img = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemBlockIO::new(&mut img);

        let mut fmt = Fat32Formatter::new(&mut io, &meta);
        fmt.format(false).expect("format failed");

        // --- FSINFO signatures (u32 LE) ---
        let bps = meta.bytes_per_sector as u64;
        let mut sec = [0u8; 512];
        io.read_at(FAT_FSINFO_SECTOR * bps, &mut sec).unwrap();

        let lead = u32::from_le_bytes(sec[0x000..0x004].try_into().unwrap());
        let stru = u32::from_le_bytes(sec[0x1E4..0x1E8].try_into().unwrap());
        let trail = u32::from_le_bytes(sec[0x1FC..0x200].try_into().unwrap());
        assert_eq!(lead, 0x41615252, "FSINFO lead signature");
        assert_eq!(stru, 0x61417272, "FSINFO struct signature");
        assert_eq!(trail, 0xAA550000, "FSINFO trail signature");

        // --- FAT0 vs FAT1 identical ---
        let fat_bytes = (meta.fat_size_sectors as usize) * (meta.bytes_per_sector as usize);
        let mut fat0 = vec![0u8; fat_bytes];
        let mut fat1 = vec![0u8; fat_bytes];
        let fat0_base = meta.fat_offset_bytes;
        let fat1_base =
            meta.fat_offset_bytes + (meta.fat_size_sectors as u64) * (meta.bytes_per_sector as u64);
        io.read_at(fat0_base, &mut fat0).unwrap();
        io.read_at(fat1_base, &mut fat1).unwrap();
        assert_eq!(fat0, fat1, "FAT0 and FAT1 must be identical");

        // Check FAT[0], FAT[1]
        let f0 = u32::from_le_bytes(fat0[0..4].try_into().unwrap());
        let f1 = u32::from_le_bytes(fat0[4..8].try_into().unwrap());
        assert_eq!(f0 & 0x0FFF_FFF8, 0x0FFF_FFF8);
        assert_eq!(f0 & 0x0000_00FF, FAT_MEDIA_DESCRIPTOR as u32);
        assert_eq!(f1 & 0x0FFF_FFFF, FAT_EOC);

        // --- Root chain reserved (root..root+FAT_PADDING) ---
        let root = meta.root_unit();
        let mut read_fat = |c: u32| -> u32 {
            let off = meta.fat_entry_offset(c, 0);
            let mut e = [0u8; 4];
            io.read_at(off, &mut e).unwrap();
            u32::from_le_bytes(e) & 0x0FFF_FFFF
        };
        assert_eq!(read_fat(root), FAT_EOC);

        // --- Root dir content: label then EOD ---
        let mut root_buf = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(root), &mut root_buf).unwrap();
        assert_eq!(root_buf[0x0B], 0x08, "volume label attribute");
        assert_eq!(root_buf[32], FAT_EOD, "EOD must follow volume label");
    }

    #[test]
    fn test_full_format_zeroes_cluster_heap_once() {
        let meta = make_meta_32mb();
        let mut img = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemBlockIO::new(&mut img);

        // Pré-remplit la heap avec 0xAA pour vérifier que full_format zère bien
        let first = meta.first_data_unit();
        let last = meta.last_data_unit();
        let start = meta.unit_offset(first);
        let end = meta.unit_offset(last) + meta.unit_size() as u64;
        let len = (end - start) as usize;
        let mut pattern = vec![0xAAu8; len];
        io.write_at(start, &pattern).unwrap();

        let mut fmt = Fat32Formatter::new(&mut io, &meta);
        fmt.format(true).expect("format(full) failed");

        // Relit cette plage → doit être 0x00
        let mut back = vec![0u8; len];
        io.read_at(start, &mut back).unwrap();
        assert!(
            back.iter().all(|&b| b == 0),
            "cluster heap must be zeroed with full_format=true"
        );
    }

    #[test]
    fn test_fsinfo_layout_and_signatures() {
        let meta = Fat32Meta::new(32 * 1024 * 1024, Some("T"));
        let mut img = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemBlockIO::new(&mut img);

        Fat32Formatter::new(&mut io, &meta).format(false).unwrap();
        let off = FAT_FSINFO_SECTOR * meta.bytes_per_sector as u64;

        // Option A: si tu as read_struct_at
        let fsi: Fat32FsInfo = io.read_struct(off).unwrap();

        assert_eq!(fsi.lead_signature, FAT_FSINFO_LEAD_SIGNATURE);
        assert_eq!(fsi.struct_signature, FAT_FSINFO_STRUCT_SIGNATURE);
        assert_eq!(fsi.trail_signature, FAT_FSINFO_TRAIL_SIGNATURE);
    }
}
