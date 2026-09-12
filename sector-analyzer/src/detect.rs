// SPDX-License-Identifier: MIT

//! Partition and filesystem detection for forensic analysis.

use rimio::RimIO;
use rimpart::{DEFAULT_SECTOR_SIZE, gpt, mbr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsKind {
    Ntfs,
    Fat,
    ExFat,
    Ext,
    Iso,
    Unknown,
}

impl FsKind {
    pub fn name(&self) -> &'static str {
        match self {
            FsKind::Ntfs => "NTFS",
            FsKind::Fat => "FAT (12/16/32)",
            FsKind::ExFat => "exFAT",
            FsKind::Ext => "EXT (2/3/4)",
            FsKind::Iso => "ISO 9660",
            FsKind::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PartitionInfo {
    pub index: usize,
    pub start_offset: u64,
    pub size_bytes: u64,
    pub part_type: String,
    pub fs: FsKind,
}

/// Detect filesystem type starting at the given byte offset.
pub fn detect_fs_at(io: &mut dyn RimIO, offset: u64) -> FsKind {
    let mut sector = [0u8; 512];
    if io.read_at(offset, &mut sector).is_ok() {
        // 1. Check for NTFS (OEM ID at 0x03)
        if &sector[3..11] == b"NTFS    " {
            return FsKind::Ntfs;
        }

        // 2. Check for exFAT (FS Name at 0x03)
        if &sector[3..11] == b"EXFAT   " {
            return FsKind::ExFat;
        }

        // 3. Check for FAT (0xAA55 end marker, bytes_per_sector in {512, 1024, 2048, 4096})
        let sig = u16::from_le_bytes([sector[510], sector[511]]);
        let bps = u16::from_le_bytes([sector[11], sector[12]]);
        let spc = sector[13];
        if sig == 0xAA55
            && (bps == 512 || bps == 1024 || bps == 2048 || bps == 4096)
            && spc.is_power_of_two()
            && sector[16] > 0
        {
            return FsKind::Fat;
        }
    }

    // 4. Check for EXT (Superblock magic 0xEF53 at offset + 1024 + 0x38)
    let mut sb_magic = [0u8; 2];
    if io.read_at(offset + 1024 + 56, &mut sb_magic).is_ok()
        && u16::from_le_bytes(sb_magic) == 0xEF53 {
            return FsKind::Ext;
        }

    // 5. Check for ISO 9660 (Descriptor at sector 16 = offset 32768)
    let mut iso_id = [0u8; 6];
    if io.read_at(offset + 16 * 2048, &mut iso_id).is_ok()
        && &iso_id[1..6] == b"CD001" {
            return FsKind::Iso;
        }

    FsKind::Unknown
}

/// Scan partition table (GPT and MBR) and detect filesystems on all partitions.
pub fn scan_partitions(io: &mut dyn RimIO) -> Vec<PartitionInfo> {
    let mut parts = Vec::new();

    // 1. Try GPT
    if let Ok((_header, gpt_entries)) = gpt::read_gpt(io) {
        for (i, entry) in gpt_entries.into_iter().enumerate() {
            if !entry.is_empty() {
                let start = entry.start_lba.get() * DEFAULT_SECTOR_SIZE;
                let end = (entry.end_lba.get() + 1) * DEFAULT_SECTOR_SIZE;
                let size = end.saturating_sub(start);
                let fs = detect_fs_at(io, start);
                let type_name = entry.kind().to_string();

                parts.push(PartitionInfo {
                    index: i,
                    start_offset: start,
                    size_bytes: size,
                    part_type: format!("GPT ({type_name})"),
                    fs,
                });
            }
        }
        if !parts.is_empty() {
            return parts;
        }
    }

    // 2. Try MBR
    if let Ok(mbr) = mbr::read_mbr(io) {
        for (i, entry) in mbr.aligned_entries().iter().enumerate() {
            if entry.part_type != 0 && entry.part_type != 0xEE {
                let start = entry.start_lba.get() as u64 * DEFAULT_SECTOR_SIZE;
                let size = entry.sectors.get() as u64 * DEFAULT_SECTOR_SIZE;
                let fs = detect_fs_at(io, start);

                parts.push(PartitionInfo {
                    index: i,
                    start_offset: start,
                    size_bytes: size,
                    part_type: format!("MBR (0x{:02X})", entry.part_type),
                    fs,
                });
            }
        }
    }

    // 3. Fallback: Superfloppy / Whole disk as filesystem
    if parts.is_empty() {
        let fs = detect_fs_at(io, 0);
        let size = io.total_size().unwrap_or(0);
        parts.push(PartitionInfo {
            index: 0,
            start_offset: 0,
            size_bytes: size,
            part_type: "Raw / Superfloppy".to_string(),
            fs,
        });
    }

    parts
}
