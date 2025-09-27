// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;
use zerocopy::IntoBytes;

pub use crate::core::formatter::*;
use crate::{
    core::cursor::ClusterMeta,
    fs::exfat::{
        constant::*,
        meta::*,
        ops,
        types::*,
        upcase::UpcaseHandle,
        utils::{self, accumulate_vbr_checksum},
    },
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
        let mut buf = Vec::with_capacity(12 * self.meta.bytes_per_sector as usize);

        let partition_offset_sectors =
            self.io.partition_offset() / (self.meta.bytes_per_sector as u64);
        let mut checksum: u32 = 0;

        // Secteur 0
        let vbr = ExFatBootSector::new_from_meta(self.meta)
            .with_partition_offset(partition_offset_sectors)
            .with_percent_in_use(self.meta.percent_in_use());
        vbr.to_raw_buffer(&mut buf);
        accumulate_vbr_checksum(&mut checksum, vbr.as_bytes(), 0);

        // Secteurs 1 à 8 - Extended Boot Sectors
        let ex = ExFatExBootSector::new();
        for i in 1..=8 {
            ex.to_raw_buffer(&mut buf);
            accumulate_vbr_checksum(&mut checksum, ex.as_bytes(), i);
        }

        // Secteurs 9-10 : OEM Parameters et Reserved (sans signature selon spec)
        let empty = vec![0u8; self.meta.bytes_per_sector as usize];
        for i in 9..=10 {
            buf.extend_from_slice(&empty);
            accumulate_vbr_checksum(&mut checksum, &empty, i);
        }

        // Secteur 11 : Checksum sector
        let sec = self.meta.bytes_per_sector as usize;
        let mut chk = vec![0u8; sec];
        for i in (0..sec).step_by(4) {
            chk[i..i + 4].copy_from_slice(&checksum.to_le_bytes());
        }
        buf.extend_from_slice(&chk);

        // Write VBR and backup
        let offset = EXFAT_VBR_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(offset, &buf)?;

        let backup_offset = EXFAT_VBR_BACKUP_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(backup_offset, &buf)?;

        Ok(())
    }

    fn write_fat_region(&mut self) -> FsFormatterResult {
        let mut buf = vec![0u8; 2 * EXFAT_ENTRY_SIZE];
        buf[0..4].copy_from_slice(&(0xFFFFFF00 | EXFAT_MEDIA_DESCRIPTOR as u32).to_le_bytes());
        buf[4..8].copy_from_slice(&EXFAT_EOC.to_le_bytes());

        let offset = self.meta.fat_entry_offset(0, 0);

        self.io.write_at(offset, &buf)?;

        let written = buf.len() as u64;
        let total_fat_bytes = self.meta.fat_size_sectors as u64 * self.meta.bytes_per_sector as u64;
        let remaining = total_fat_bytes.saturating_sub(written);
        self.io.zero_fill(offset + written, remaining as usize)?;
        Ok(())
    }

    fn write_bitmap(&mut self) -> FsFormatterResult {
        // Initialise le cluster bitmap à zéro
        let offset = self.meta.unit_offset(self.meta.bitmap_cluster);
        self.io.zero_fill(offset, self.meta.unit_size())?;
        Ok(())
    }

    fn write_upcase_table(&mut self) -> FsFormatterResult<(u64, u32)> {
        let offset = self.meta.unit_offset(self.meta.upcase_cluster);

        let upcase = UpcaseHandle::from_flavor(&self.meta.upcase_flavor);

        self.io
            .write_block_best_effort(offset, upcase.as_bytes(), self.meta.unit_size())?;

        Ok((upcase.len() as u64, upcase.checksum()))
    }

    fn write_root_dir_cluster(
        &mut self,
        upcase_len: u64,
        upcase_checksum: u32,
    ) -> FsFormatterResult {
        let mut buf = Vec::with_capacity(self.meta.unit_size());

        ExFatBitmapEntry::new(
            self.meta.bitmap_cluster,
            self.meta.bitmap_size_bytes, // Size in BYTES (1 bit per cluster, rounded up)
        )
        .to_raw_buffer(&mut buf);
        ExFatUpcaseEntry::new(self.meta.upcase_cluster, upcase_len, upcase_checksum)
            .to_raw_buffer(&mut buf);
        ExFatVolumeLabelEntry::new(self.meta.volume_label).to_raw_buffer(&mut buf);
        if self.meta.volume_guid.is_some() {
            ExFatGuidEntry::new(self.meta.volume_guid.unwrap()).to_raw_buffer(&mut buf);
        }

        ExFatEodEntry::new().to_raw_buffer(&mut buf);

        let offset = self.meta.unit_offset(self.meta.root_unit());
        self.io.write_at(offset, &buf)?;

        Ok(())
    }

    /// Alloue tous les clusters système (bitmap, upcase, root) d'un coup
    fn allocate_system_clusters(&mut self) -> FsFormatterResult {
               // vecteur [start .. start+len)
        let build_chain =
            |start: u32, len: u32| -> Vec<u32> { (0..len).map(|i| start + i).collect() };
        let bitmap_chain = build_chain(self.meta.bitmap_cluster, self.meta.bitmap_clusters());
        let upcase_chain = build_chain(self.meta.upcase_cluster, self.meta.upcase_clusters());
        let root_chain = build_chain(self.meta.root_unit(), self.meta.root_clusters());

        // Écrit la FAT (chaînage + EOC) et le bitmap (bits à 1) pour chaque chaîne
        for ch in [&bitmap_chain, &upcase_chain, &root_chain] {
            ops::write_fat_chain(self.io, self.meta, ch)?;
            utils::write_bitmap(self.io, self.meta, ch)?;
        }

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

impl<'a, IO: BlockIO + ?Sized> FsFormatter for ExFatFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult {
        self.write_vbr()?;
        self.write_fat_region()?;
        self.write_bitmap()?; // Initialise le bitmap d'abord
        let (upcase_len, upcase_checksum) = self.write_upcase_table()?; // Écrit la table upcase
        self.write_root_dir_cluster(upcase_len, upcase_checksum)?; // Écrit le répertoire racine
        self.allocate_system_clusters()?; // Alloue tous les clusters système d'un coup
        if full_format {
            self.zero_cluster_heap()?;
        }
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::{
        core::cursor::ClusterMeta,
        fs::exfat::{constant::EXFAT_FIRST_CLUSTER, prelude::*},
    };

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

        // Secteur 0 : VBR principal avec jump boot et FS name
        let sector0 = &vbr_sectors[0..512];
        assert_eq!(
            &sector0[0..3],
            &[0xEB, 0x76, 0x90],
            "Incorrect JumpBoot (sector 0)"
        );
        assert_eq!(&sector0[3..11], b"EXFAT   ", "Incorrect FS name (sector 0)");
        assert_eq!(
            &sector0[510..512],
            &[0x55, 0xAA],
            "Missing signature (sector 0)",
        );
        hexdump("VBR Sector 0", sector0);

        // Secteurs 1-8 : Extended Boot Sectors (avec signature requise)
        for i in 1..=8 {
            let sector = &vbr_sectors[i * 512..(i + 1) * 512];
            assert_eq!(
                &sector[510..512],
                &[0x55, 0xAA],
                "Missing signature (extended boot sector {i})",
            );
            hexdump(&format!("Extended Boot Sector {i}"), sector);
        }

        // Secteurs 9-10 : OEM Parameters et Reserved (SANS signature selon spec Microsoft)
        for i in 9..=10 {
            let sector = &vbr_sectors[i * 512..(i + 1) * 512];
            // Ces secteurs ne doivent PAS avoir de signature selon la spec
            assert_eq!(
                &sector[510..512],
                &[0x00, 0x00],
                "Unexpected signature in OEM/Reserved sector {i} (should be 0x0000)",
            );
            let sector_type = if i == 9 { "OEM Parameters" } else { "Reserved" };
            hexdump(&format!("{sector_type} Sector {i}"), sector);
        }

        let checksum_sector = &vbr_sectors[11 * 512..12 * 512];
        let checksum = u32::from_le_bytes(checksum_sector[0..4].try_into().unwrap());
        // Selon la spécification exFAT, le secteur checksum répète le checksum
        // sur 510 bytes (512 - 2), ce qui fait 127 mots complets de 4 bytes + 2 bytes restants
        // Nous ne vérifions que les mots complets de 4 bytes
        let complete_words = (512 - 2) / 4; // 510 / 4 = 127
        for i in 0..complete_words {
            let chunk = &checksum_sector[i * 4..(i + 1) * 4];
            assert_eq!(
                u32::from_le_bytes(chunk.try_into().unwrap()),
                checksum,
                "Invalid repeated checksum at index {i}",
            );
        }
        // Vérifier que les 2 derniers bytes avant la signature sont les 2 premiers bytes du checksum
        let partial_word_start = complete_words * 4;
        assert_eq!(
            &checksum_sector[partial_word_start..partial_word_start + 2],
            &checksum.to_le_bytes()[0..2],
            "Invalid partial checksum word"
        );
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
        // même setup
        let meta = ExFatMeta::new(SIZE_BYTES, Some("FATTEST"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();

        // lire toute la FAT
        let fat_bytes = meta.fat_size_sectors * meta.bytes_per_sector as u32;
        let mut fat_buf = vec![0u8; fat_bytes as usize];
        io.read_at(meta.fat_offset_bytes, &mut fat_buf).unwrap();

        // helpers
        let entry_at = |clus: u32| -> u32 {
            let off = (clus as usize) * 4;
            u32::from_le_bytes(fat_buf[off..off + 4].try_into().unwrap())
        };

        // 0: media, 1: réservé
        assert_eq!(
            entry_at(0),
            0xFFFFFFF8,
            "Cluster 0 should be media descriptor"
        );
        assert_eq!(entry_at(1), 0xFFFFFFFF, "Cluster 1 should be reserved");

        // bornes dynamiques des zones système
        let bm_first = meta.bitmap_cluster;
        let bm_last = bm_first + meta.bitmap_clusters() - 1;

        let uc_first = meta.upcase_cluster;
        let uc_last = uc_first + meta.upcase_clusters() - 1;

        let rt_first = meta.root_cluster;
        let rt_last = rt_first + meta.root_clusters() - 1;

        // Tout cluster système doit être "utilisé" (≠ 0)
        for c in bm_first..=bm_last {
            assert_ne!(
                entry_at(c),
                0x0000_0000,
                "Bitmap cluster {c} must be allocated"
            );
        }
        for c in uc_first..=uc_last {
            assert_ne!(
                entry_at(c),
                0x0000_0000,
                "Upcase cluster {c} must be allocated"
            );
        }
        for c in rt_first..=rt_last {
            assert_ne!(
                entry_at(c),
                0x0000_0000,
                "Root cluster {c} must be allocated"
            );
        }

        // Quelques clusters immédiatement après le dernier système doivent être libres (== 0)
        let sys_end = bm_last.max(uc_last).max(rt_last);
        let last_clus = EXFAT_FIRST_CLUSTER + meta.cluster_count - 1;
        let free_range_end = (sys_end + 3).min(last_clus);
        for c in (sys_end + 1)..=free_range_end {
            assert_eq!(entry_at(c), 0x0000_0000, "Cluster {c} should be free");
        }

        hexdump("FAT Region", &fat_buf[..64]);
    }

    #[test]
    fn test_exfat_bitmap() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("BITMAPT"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();

        // lire la bitmap complète (sur N clusters si besoin)
        let bm_first = meta.bitmap_cluster;
        let bm_clusters = meta.bitmap_clusters();
        let cs = meta.unit_size();
        let mut bitmap = vec![0u8; meta.bitmap_size_bytes as usize];

        for i in 0..bm_clusters as usize {
            let off = meta.unit_offset(bm_first + i as u32);
            // quantité utile à copier dans le buffer final
            let begin = i * cs;
            if begin >= bitmap.len() {
                break;
            }
            let take = (bitmap.len() - begin).min(cs);
            io.read_at(off, &mut bitmap[begin..begin + take]).unwrap();
            if take < cs {
                break;
            } // dernier morceau partiel
        }

        // helpers
        let bit_is_set = |clus: u32| -> bool {
            let (byte_index, bit_mask) = meta.bitmap_entry_offset(clus);
            (bitmap[byte_index] & bit_mask) != 0
        };

        // zones système dynamiques
        let bm_last = bm_first + bm_clusters - 1;
        let uc_first = meta.upcase_cluster;
        let uc_last = uc_first + meta.upcase_clusters() - 1;
        let rt_first = meta.root_cluster;
        let rt_last = rt_first + meta.root_clusters() - 1;

        // La bitmap doit marquer utilisés: bitmap, upcase, root
        for c in bm_first..=bm_last {
            assert!(
                bit_is_set(c),
                "Bitmap bit for bitmap cluster {c} must be set"
            );
        }
        for c in uc_first..=uc_last {
            assert!(
                bit_is_set(c),
                "Bitmap bit for upcase cluster {c} must be set"
            );
        }
        for c in rt_first..=rt_last {
            assert!(bit_is_set(c), "Bitmap bit for root cluster {c} must be set");
        }

        // Un cluster immédiatement après les systèmes devrait être libre
        let sys_end = bm_last.max(uc_last).max(rt_last);
        let probe = sys_end + 1;
        if (probe - EXFAT_FIRST_CLUSTER) < meta.cluster_count {
            assert!(
                !bit_is_set(probe),
                "Bitmap bit for cluster {probe} should be clear"
            );
        }

        hexdump("Allocation Bitmap", &bitmap[..bitmap.len().min(32)]);
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
        io.read_at(meta.unit_offset(meta.root_unit()), &mut root)
            .unwrap();

        // exfat-fs order: Bitmap, Upcase, Volume Label, GUID, EOD
        assert_eq!(&root[0..2], &[0x81, 0x00]); // Bitmap
        assert_eq!(&root[32..34], &[0x82, 0x00]); // Upcase
        assert_eq!(&root[64..66], &[0x83, 0x08]); // Volume Label ("UPCASETB" -> 8 chars)
        assert_eq!(&root[96..98], &[0xA0, 0x00]); // GUID
        assert_eq!(&root[128..130], &[0x00, 0x00]); // EOD

        hexdump("Root Directory", &root[..160]);
    }

    #[test]
    fn test_exfat_format() {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("TESTVOL"));
        let mut buffer = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemBlockIO::new(&mut buffer);

        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("ExFAT format failed");

        let mut vbr = [0u8; 512];
        io.read_at(0, &mut vbr).unwrap();
        hexdump("Sector 0 (VBR)", &vbr);

        let mut root = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.root_unit()), &mut root)
            .unwrap();
        hexdump("Root Cluster", &root[..128]);

        let mut bitmap = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.bitmap_cluster), &mut bitmap)
            .unwrap();
        hexdump("Allocation Bitmap", &bitmap[..64.min(bitmap.len())]);

        let fat_bytes = (2..10)
            .flat_map(|c| {
                let offset = meta.fat_entry_offset(c, 0);
                let mut entry = [0u8; 4];
                io.read_at(offset, &mut entry).unwrap();
                entry
            })
            .collect::<Vec<u8>>();
        hexdump("FAT[2..10]", &fat_bytes);
    }
}
