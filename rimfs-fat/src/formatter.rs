pub use crate::core::formatter::*;
use alloc::vec::Vec;
use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::{
    core::fat::*,
    {boot_code, constant::*, meta::*, types::*},
};

/// FatFormatter:
/// - Fast + valid formatter.
/// - Prepares VBR, FSINFO, FAT initial entries, and Root Directory.
/// - Does not pre-allocate full FAT chains (injector is responsible for filling remaining data).
/// - Suitable for image generators (rimgen), bootable FS, and validated FAT structures.
pub struct FatFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a FatMeta,
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for FatFormatter<'a, IO> {
    fn format(&mut self, full_format: bool) -> FsFormatterResult {
        if full_format {
            crate::core::formatter::zero_cluster_heap(self.io, self.meta)?;
        }

        self.write_vbr()?;
        self.write_fsinfo()?;
        self.write_fat_region()?;

        self.write_root_dir_cluster()?;
        self.allocate_system_clusters()?;

        self.io.flush()?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FatFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a FatMeta) -> Self {
        Self { io, meta }
    }

    fn write_vbr(&mut self) -> FsFormatterResult {
        let mut vbr = FatVbr::from_meta(self.meta);
        let bps = self.meta.bytes_per_sector as u64;

        let hidden_sectors = (self.io.partition_offset() / bps) as u32;
        vbr = vbr.with_hidden_sectors(hidden_sectors);

        if self.meta.bits == 32 {
            vbr = vbr.with_boot_code(32, &boot_code::FAT32_BOOT_CODE_P0);
        }

        // Write VBR to sector 0 (Main)
        let offset = FAT_VBR_SECTOR * bps;
        self.io.write_at(offset, vbr.as_bytes())?;

        // Write backup VBR (Sector 6)
        let backup_offset = FAT_VBR_BACKUP_SECTOR * bps;
        self.io.write_at(backup_offset, vbr.as_bytes())?;

        if self.meta.bits == 32 {
            // Write Sector 2 (Main)
            let mut p2 = [0u8; 512];
            p2[0..510].copy_from_slice(&boot_code::FAT32_BOOT_CODE_P2);
            p2[510..512].copy_from_slice(&FAT_SIGNATURE.to_le_bytes());
            self.io.write_at(2 * bps, &p2)?;

            // Write backup Sector 2 (Sector 8)
            self.io.write_at(8 * bps, &p2)?;
        }

        Ok(())
    }

    fn write_fsinfo(&mut self) -> FsFormatterResult {
        if self.meta.bits != 32 {
            return Ok(());
        }
        let fsinfo = FatFsInfo::from_meta(self.meta);

        // Write main FSINFO
        let offset = FAT_FSINFO_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(offset, fsinfo.as_bytes())?;

        // Write backup FSINFO
        let backup_offset = FAT_FSINFO_BACKUP_SECTOR * self.meta.bytes_per_sector as u64;
        self.io.write_at(backup_offset, fsinfo.as_bytes())?;

        Ok(())
    }

    fn write_fat_region(&mut self) -> FsFormatterResult {
        let fat_size_bytes = self.meta.fat_size_sectors as u64 * self.meta.bytes_per_sector as u64;
        let eoc_val = self.meta.entry_mask();
        let media_val = eoc_val & !0x7; // Media Descriptor + EOC markers

        // 1. Zero-fill ALL FAT copies first
        for fat_index in 0..self.meta.num_fats {
            let offset = self.meta.fat_table_offset(fat_index);
            self.io.zero_fill(offset, fat_size_bytes as usize)?;
        }

        // 2. Write the first two reserved entries (Media Descriptor + EOC)
        // FatDriver handles mirroring automatically.
        let mut driver = FatDriver::new(self.meta);
        driver.write_entry(self.io, 0, media_val)?;
        driver.write_entry(self.io, 1, eoc_val)?;

        Ok(())
    }

    fn write_root_dir_cluster(&mut self) -> FsFormatterResult {
        let mut buf = Vec::with_capacity(self.meta.unit_size());

        FatEntries::volume_label(self.meta.volume_label)
            .with_integrity_calculated(self.meta)
            .to_raw_buffer(&mut buf);
        FatEodEntry::new().to_raw_buffer(&mut buf);

        let offset = self.meta.unit_offset(self.meta.root_unit());
        self.io.write_at(offset, &buf)?;

        Ok(())
    }

    fn allocate_system_clusters(&mut self) -> FsFormatterResult {
        let root = self.meta.root_unit();
        let mut rl = RunList::new();
        rl.push(Run {
            start: root as u64,
            length: 1,
        });

        let mut driver = FatDriver::new(self.meta);
        driver.write_run_list(self.io, &rl)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Validate;
    use crate::core::checker::{FsChecker, VerifyReport};

    fn make_meta_32mb() -> FatMeta {
        const SIZE: u64 = 32 * 1024 * 1024;
        FatMeta::new_fat32(SIZE, Some("TESTFS")).unwrap()
    }

    #[test]
    fn test_format_writes_vbr_fsinfo_fat_and_root() {
        let meta = make_meta_32mb();
        let mut img = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemRimIO::new(&mut img);

        let mut fmt = FatFormatter::new(&mut io, &meta);
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
        assert!(
            meta.is_eoc(f1 & 0x0FFF_FFFF),
            "FAT[1] must be an EOC marker"
        );

        // --- Root chain reserved (root..root+FAT_PADDING) ---
        let root = meta.root_unit();
        let mut read_fat = |c: u32| -> u32 {
            let off = meta.fat_table_offset(0) + c as u64 * 4;
            let mut e = [0u8; 4];
            io.read_at(off, &mut e).unwrap();
            u32::from_le_bytes(e) & 0x0FFF_FFFF
        };
        assert!(
            meta.is_eoc(read_fat(root)),
            "Root unit must be marked as EOC"
        );

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
        let mut io = MemRimIO::new(&mut img);

        // Pre-fill the heap with 0xAA to verify that full_format properly zeroes it out
        let first = meta.first_data_unit();
        let last = meta.last_data_unit();
        let start = meta.unit_offset(first);
        let end = meta.unit_offset(last) + meta.unit_size() as u64;
        let len = (end - start) as usize;
        let pattern = vec![0xAAu8; len];
        io.write_at(start, &pattern).unwrap();

        let mut fmt = FatFormatter::new(&mut io, &meta);
        fmt.format(true).expect("format(full) failed");

        // Read this range back → must be 0x00
        let mut back = vec![0u8; len];
        io.read_at(start, &mut back).unwrap();
        assert!(
            back.iter().all(|&b| b == 0),
            "cluster heap must be zeroed with full_format=true"
        );
    }

    #[test]
    fn test_fsinfo_layout_and_signatures() {
        let meta = FatMeta::new_fat32(32 * 1024 * 1024, Some("T")).unwrap();
        let mut img = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemRimIO::new(&mut img);

        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let off = FAT_FSINFO_SECTOR * meta.bytes_per_sector as u64;

        // Use read_struct to verify the header directly
        let fsi: FatFsInfo = io.read_struct(off).unwrap();

        assert_eq!(fsi.lead_signature, FAT_FSINFO_LEAD_SIGNATURE);
        assert_eq!(fsi.struct_signature, FAT_FSINFO_STRUCT_SIGNATURE);
        assert_eq!(fsi.trail_signature, FAT_FSINFO_TRAIL_SIGNATURE);
    }
    #[test]
    fn test_format_fat12_basic() {
        // 10 MB FAT12
        let size = 10 * 1024 * 1024;
        let meta = FatMeta::new_fat12(size, Some("FAT12")).unwrap();
        let mut img = vec![0u8; size as usize];
        let mut io = MemRimIO::new(&mut img);

        let mut fmt = FatFormatter::new(&mut io, &meta);
        fmt.format(false).expect("FAT12 format failed");

        // Verify VBR at 0
        let mut vbr_buf = [0u8; 512];
        io.read_at(0, &mut vbr_buf).unwrap();
        assert_eq!(vbr_buf[510], 0x55);
        assert_eq!(vbr_buf[511], 0xAA);

        // Verify FAT start
        let fat_start = meta.fat_offset_bytes;
        println!("FAT12: VBR=0, FAT Start={}", fat_start);
        assert!(fat_start >= 512, "FAT must not overwrite VBR");

        // Verify that VBR matches what we expect
        let vbr: FatVbr = io.read_struct(0).unwrap();
        vbr.validate(&meta).expect("FAT12 VBR invalid");

        // Verify using checker
        let mut rep = VerifyReport::default();
        let mut checker = crate::checker::FatChecker::new(&mut io, &meta);
        checker.check_boot(&Default::default(), &mut rep).unwrap();

        for finding in &rep.findings {
            println!("Finding: {:?}", finding);
        }
        assert!(!rep.has_error(), "Checker found errors: {:?}", rep.findings);
        assert!(
            rep.findings.iter().any(|f| f.code == "VBR.OK"),
            "Expected VBR.OK finding"
        );
    }

    #[test]
    fn test_format_fat16_basic() {
        let size = 50 * 1024 * 1024; // 50MB
        let meta = FatMeta::new_fat16(size, Some("FAT16")).unwrap();
        let mut img = vec![0u8; size as usize];
        let mut io = MemRimIO::new(&mut img);

        FatFormatter::new(&mut io, &meta).format(false).unwrap();

        // Verify VBR
        let mut val = [0u8; 2];
        io.read_at(510, &mut val).unwrap();
        assert_eq!(val, [0x55, 0xAA], "FAT16 VBR signature missing");

        // Verify using struct
        let vbr: FatVbr = io.read_struct(0).unwrap();
        vbr.validate(&meta).expect("FAT16 VBR invalid");

        // Verify using checker
        let mut rep = VerifyReport::default();
        let mut checker = crate::checker::FatChecker::new(&mut io, &meta);
        checker.check_boot(&Default::default(), &mut rep).unwrap();
        assert!(!rep.has_error(), "Checker found errors: {:?}", rep.findings);
        assert!(
            rep.findings.iter().any(|f| f.code == "VBR.OK"),
            "Expected VBR.OK finding"
        );
    }

    #[test]
    fn test_fat12_backup_vbr_overwrite() {
        // This test investigates the Backup VBR vs FAT overlap issue in FAT12.
        // If Reserved=2, FAT starts at Sector 2. Backup VBR is Sector 6.
        // So Backup VBR is written inside FAT area.
        // Formatter writes VBR, then FAT. FAT zeroing should wipe Backup VBR.

        let size = 16 * 1024 * 1024;
        let meta = FatMeta::new_fat12(size, Some("OVRLAP")).unwrap();
        let mut img = vec![0u8; size as usize];
        let mut io = MemRimIO::new(&mut img);

        let mut fmt = FatFormatter::new(&mut io, &meta);
        fmt.format(false).unwrap();

        // Check Sector 6
        let mut sec6 = [0u8; 512];
        io.read_at(6 * 512, &mut sec6).unwrap();

        // It should NOT match VBR because FAT zeroing wiped it (or it's FAT data)
        // Unless reserved sectors > 6.
        if meta.fat_offset_bytes <= 6 * 512 {
            // Inside FAT
            // It shouldn't be VBR signature unless checking fail
            assert_ne!(
                sec6[510..512],
                [0x55, 0xAA],
                "Sector 6 should be overwritten by FAT in FAT12/16 default layout"
            );
        } else {
            // Outside FAT (if reserved sectors large)
            // Then it might be Backup VBR
            assert_eq!(sec6[510..512], [0x55, 0xAA]);
        }
    }
}
