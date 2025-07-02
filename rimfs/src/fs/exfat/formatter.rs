// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;
use zerocopy::IntoBytes;

pub use crate::core::formatter::*;
use crate::fs::exfat::{
    constant::*,
    meta::*,
    types::*,
    utils::{self, to_unicode_upper_or_ff},
};

/// ExFatFormatter:
/// - Valid formatter for ExFAT.
/// - Prepares VBR, FAT region, Allocation Bitmap, Root Dir.
/// - No pre-allocation of FAT chains → injector does that.
pub struct ExFatFormatter<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
}

impl<'a, IO: BlockIO + ?Sized> ExFatFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        Self { io, meta }
    }

    fn write_vbr(&mut self) -> FsFormatterResult {
        let mut buf = Vec::with_capacity(12 * self.meta.sector_size as usize);

        let partition_offset_sectors = self.io.partition_offset() / (self.meta.sector_size as u64);
        let mut checksum: u32 = 0;

        let percent_in_use =
            (self.meta.first_data_unit() * 100 / self.meta.cluster_count as u32).min(100);

        // Secteur 0
        let vbr = ExFatBootSector::new_from_meta(self.meta)
            .with_partition_offset(partition_offset_sectors)
            .with_percent_in_use(percent_in_use.try_into().unwrap());
        vbr.to_raw_buffer(&mut buf);
        accumulate_checksum(&mut checksum, vbr.as_bytes(), 0);

        // Secteurs 1 à 10
        let empty = ExFatExBootSector::new();
        for i in 1..=8 {
            empty.to_raw_buffer(&mut buf);
            accumulate_checksum(&mut checksum, empty.as_bytes(), i);
        }

        for i in 9..=10 {
            let zero = vec![0u8; self.meta.sector_size as usize];
            buf.extend_from_slice(&zero);
            accumulate_checksum(&mut checksum, &zero, i);
        }

        // Secteur 11 : Checksum sector
        let repeated = checksum.to_le_bytes();
        for _ in 0..(self.meta.sector_size as usize / 4) {
            buf.extend_from_slice(&repeated);
        }

        // Write VBR and backup
        let offset = EXFAT_VBR_SECTOR * self.meta.sector_size as u64;
        self.io.write_at(offset, &buf)?;

        let backup_offset = EXFAT_VBR_BACKUP_SECTOR * self.meta.sector_size as u64;
        self.io.write_at(backup_offset, &buf)?;

        Ok(())
    }

    fn write_fat_region(&mut self) -> FsFormatterResult {
        let mut buf = vec![0u8; 2 * EXFAT_ENTRY_SIZE];
        buf[0..4].copy_from_slice(&(0xFFFFFF00 | EXFAT_MEDIA_DESCRIPTOR as u32).to_le_bytes());
        buf[4..8].copy_from_slice(&EXFAT_EOC.to_le_bytes());
        self.io.write_at(self.meta.fat_offset, &buf)?;

        let written = buf.len() as u64;
        let total_fat_bytes = self.meta.fat_size as u64 * self.meta.sector_size as u64;
        let remaining = total_fat_bytes.saturating_sub(written);
        self.io
            .zero_fill(self.meta.fat_offset + written, remaining as usize)?;
        Ok(())
    }

    fn write_bitmap(&mut self) -> FsFormatterResult {
        let offset = self.meta.unit_offset(self.meta.bitmap_cluster);
        self.io.zero_fill(offset, self.meta.unit_size())?;

        let cluster_chain = [self.meta.bitmap_cluster];
        utils::write_fat_chain(self.io, self.meta, &cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &cluster_chain)?;

        for cluster in self.meta.root_unit()..self.meta.first_data_unit() {
            let cluster_chain = [cluster];
            utils::write_fat_chain(self.io, self.meta, &cluster_chain)?;
            utils::write_bitmap(self.io, self.meta, &cluster_chain)?;
        }

        Ok(())
    }

    fn write_upcase_table(&mut self) -> FsFormatterResult<u32> {
        let offset = self.meta.unit_offset(self.meta.upcase_cluster);

        let mut checksum: u32 = 0;

        self.io
            .write_block_best_effort(offset, &EXFAT_UPCASE_TABLE, self.meta.unit_size())?;

        accumulate_checksum_word(&mut checksum, &EXFAT_UPCASE_TABLE);

        // self.io
        //     .write_chunks_streamed::<2, _>(offset, EXFAT_UPCASE_TABLE_SIZE, self.meta.unit_size(), |i| {
        //         let upper = to_unicode_upper_or_ff(i as u16);
        //         let bytes = upper.to_le_bytes();
        //         accumulate_checksum_word(&mut checksum, &bytes);
        //         bytes
        //     })?;

        let cluster_chain = [self.meta.upcase_cluster];
        utils::write_fat_chain(self.io, self.meta, &cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &cluster_chain)?;

        Ok(checksum)
    }

    fn write_root_dir_cluster(&mut self, upcase_checksum: u32) -> FsFormatterResult {
        let mut buf = Vec::with_capacity(self.meta.unit_size());
        ExFatVolumeLabelEntry::new(self.meta.volume_label).to_raw_buffer(&mut buf);
        ExFatBitmapEntry::new(
            self.meta.bitmap_cluster,
            self.meta.cluster_count.div_ceil(8) as u64,
        )
        .to_raw_buffer(&mut buf);
        ExFatUpcaseEntry::new(self.meta.upcase_cluster, EXFAT_UPCASE_CHECKSUM)
            .to_raw_buffer(&mut buf);
        let guid = generate_volume_id_128().to_le_bytes(); // [u8; 16]
        ExFatGuidEntry::new(guid).to_raw_buffer(&mut buf);
        ExFatEodEntry::new().to_raw_buffer(&mut buf);

        let offset = self.meta.unit_offset(self.meta.root_unit());
        self.io.write_at(offset, &buf)?;

        let cluster_chain = [self.meta.root_unit()];
        utils::write_fat_chain(self.io, self.meta, &cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &cluster_chain)?;

        Ok(())
    }

    fn zero_cluster_heap(&mut self) -> FsFormatterResult {
        // Cluster heap starts after FAT + Bitmap → from first_data_cluster()
        for cluster in self.meta.first_data_unit()..self.meta.last_data_unit() {
            let offset = self.meta.unit_offset(cluster);
            self.io.zero_fill(offset, self.meta.unit_size())?;
        }

        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsFormatter for ExFatFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult {
        self.write_vbr()?;
        self.write_fat_region()?;
        self.write_bitmap()?;
        let upcase_checksum = self.write_upcase_table()?;
        self.write_root_dir_cluster(upcase_checksum)?;
        if full_format {
            self.zero_cluster_heap()?;
        }
        self.io.flush()?;
        Ok(())
    }
}

fn accumulate_checksum(sum: &mut u32, data: &[u8], sector_index: usize) {
    for (i, &byte) in data.iter().enumerate() {
        if sector_index == 0 && (i == 106 || i == 107 || i == 112) {
            continue;
        }
        *sum = sum.rotate_right(1).wrapping_add(byte as u32);
    }
}

fn accumulate_checksum_word(sum: &mut u32, bytes: &[u8]) {
    for &byte in bytes {
        *sum = sum.rotate_right(1).wrapping_add(byte as u32);
    }
}

#[cfg(test)]
mod test {
    use crate::fs::exfat::prelude::*;

    fn hexdump(label: &str, data: &[u8]) {
        println!("--- {label} ---");
        for (i, chunk) in data.chunks(16).enumerate() {
            print!("{:04X}: ", i * 16);
            for b in chunk {
                print!("{b:02X} ");
            }
            for _ in chunk.len()..16 {
                print!("   ");
            }
            print!(" | ");
            for &b in chunk {
                let c = if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                };
                print!("{c}");
            }
            println!();
        }
    }

    const SIZE_MB: u64 = 8;

    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    #[test]
    fn test_exfat_vbr() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("TESTVOL"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("ExFAT format failed");

        let mut vbr_sectors = [0u8; 512 * 12];
        io.read_at(0, &mut vbr_sectors).unwrap();

        for i in 0..11 {
            let sector = &vbr_sectors[i * 512..(i + 1) * 512];
            assert_eq!(
                &sector[0..3],
                &[0xEB, 0x76, 0x90],
                "Incorrect JumpBoot (sector {i})"
            );
            assert_eq!(
                &sector[3..11],
                b"EXFAT   ",
                "Incorrect FS name (sector {i})"
            );
            assert_eq!(
                &sector[510..512],
                &[0x55, 0xAA],
                "Missing signature (sector {i})",
            );
            hexdump(&format!("VBR Sector {i}"), sector);
        }

        let checksum_sector = &vbr_sectors[11 * 512..12 * 512];
        let checksum = u32::from_le_bytes(checksum_sector[0..4].try_into().unwrap());
        for i in 0..(512 / 4) {
            let chunk = &checksum_sector[i * 4..(i + 1) * 4];
            assert_eq!(
                u32::from_le_bytes(chunk.try_into().unwrap()),
                checksum,
                "Invalid repeated checksum at index {i}",
            );
        }
        hexdump("VBR Checksum Sector (11)", checksum_sector);

        println!("VBR checksum = 0x{checksum:08X}");

        // === Lecture backup VBR (secteurs 12 à 23) ===
        let mut backup_sectors = [0u8; 512 * 12];
        let backup_offset = 12 * 512;
        io.read_at(backup_offset as u64, &mut backup_sectors)
            .unwrap();

        for i in 0..12 {
            let ref_main = &vbr_sectors[i * 512..(i + 1) * 512];
            let ref_backup = &backup_sectors[i * 512..(i + 1) * 512];
            assert_eq!(ref_main, ref_backup, "Mismatch VBR vs Backup at sector {i}");
        }
    }

    #[test]
    fn test_exfat_fat_region() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("FATTEST"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();

        let fat_bytes = meta.fat_size * meta.sector_size as u32;
        let mut fat_buf = vec![0u8; fat_bytes as usize];
        io.read_at(meta.fat_offset, &mut fat_buf).unwrap();

        for cluster in 0..8 {
            let entry = u32::from_le_bytes(
                fat_buf[(cluster * 4) as usize..(cluster * 4 + 4) as usize]
                    .try_into()
                    .unwrap(),
            );
            match cluster {
                0 => assert_eq!(entry, 0xFFFFFFF8, "Cluster 0 should be media descriptor"),
                1 => assert_eq!(entry, 0xFFFFFFFF, "Cluster 1 should be reserved"),
                2 => assert_eq!(entry, 0xFFFFFFFF, "Cluster 2 (bitmap) should be allocated"),
                3 => assert_eq!(entry, 0xFFFFFFFF, "Cluster 3 (upcase) should be allocated"),
                4 => assert_eq!(
                    entry, 0xFFFFFFFF,
                    "Cluster 4 (root dir) should be allocated"
                ),
                _ => assert_eq!(entry, 0x00000000, "Cluster {cluster} should be free"),
            }
        }

        hexdump("FAT Region", &fat_buf[..64]);
    }

    #[test]
    fn test_exfat_bitmap() {
        const SIZE_MB: u64 = 8;
        const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

        let meta = ExFatMeta::new(SIZE_BYTES, Some("BITMAPT"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();

        let bitmap_offset = meta.unit_offset(meta.bitmap_cluster);
        let mut bitmap = vec![0u8; meta.unit_size()];
        io.read_at(bitmap_offset, &mut bitmap).unwrap();

        for cluster in 2..=4 {
            let byte_index = (cluster / 8) as usize;
            let bit_index = cluster % 8;
            let is_set = (bitmap[byte_index] >> bit_index) & 1;
            assert_eq!(
                is_set, 1,
                "Cluster {cluster} should be marked as used in the bitmap"
            );
        }

        let byte_index = 5 / 8;
        let bit_index = 5;
        let is_set = (bitmap[byte_index] >> bit_index) & 1;
        assert_eq!(is_set, 0, "Cluster 5 should be free in the bitmap");

        hexdump("Allocation Bitmap", &bitmap[..32]);
    }

    #[test]
    fn test_exfat_upcase_table() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("UPCASETB"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();

        let upcase_offset = meta.unit_offset(meta.upcase_cluster);
        let mut upcase_buf = vec![0u8; 512];
        io.read_at(upcase_offset, &mut upcase_buf).unwrap();

        let value = u16::from_le_bytes(upcase_buf[0..2].try_into().unwrap());
        assert_eq!(value, 0x0000); // Should be start of upcase table
        hexdump("Upcase Table", &upcase_buf[..64]);
    }

    #[test]
    fn test_exfat_root_dir() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("UPCASETB"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut root = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.root_cluster), &mut root)
            .unwrap();

        assert_eq!(&root[0..2], &[0x81, 0x00]); // Bitmap
        assert_eq!(&root[32..34], &[0x82, 0x00]); // Upcase
        assert_eq!(&root[64..66], &[0x83, 0x0B]); // Volume Label
        assert_eq!(&root[96..98], &[0x00, 0x00]); // EOD

        hexdump("Root Directory", &root[..128]);
    }

    #[test]
    fn test_exfat_format() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("TESTVOL"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("ExFAT format failed");

        ExFatChecker::new(&mut io, &meta)
            .check_all()
            .expect("Checker failed");

        let mut vbr = [0u8; 512];
        io.read_at(0, &mut vbr).unwrap();
        hexdump("Sector 0 (VBR)", &vbr);

        let mut root = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.root_cluster), &mut root)
            .unwrap();
        hexdump("Root Cluster", &root[..128]);

        let mut bitmap = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.bitmap_cluster), &mut bitmap)
            .unwrap();
        hexdump("Allocation Bitmap", &bitmap[..64.min(bitmap.len())]);

        let fat_bytes = (2..10)
            .flat_map(|c| {
                let offset = meta.fat_entry_offset(c);
                let mut entry = [0u8; 4];
                io.read_at(offset, &mut entry).unwrap();
                entry
            })
            .collect::<Vec<u8>>();
        hexdump("FAT[2..10]", &fat_bytes);
    }
}
