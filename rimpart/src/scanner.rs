// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};

use rimio::prelude::*;

use crate::{
    DEFAULT_SECTOR_SIZE,
    errors::*,
    gpt::{self, GptEntry, GptHeader},
    guids::GptPartitionKind,
    io_ext::BlockIOLbaExt,
    mbr::{self, Mbr, MbrKind, PROTECTIVE_GPT},
};

/// Options pour le scan disque
#[derive(Clone, Copy, Debug)]
pub struct DiskScanOptions {
    /// Taille logique d’un secteur (LBA) en octets
    pub sector_size: u64,
    /// Valider le CRC du header/entries GPT
    pub validate_crc: bool,
    /// Vérifier les bornes des partitions (overlaps, etc.)
    pub validate_bounds: bool,
}

impl Default for DiskScanOptions {
    fn default() -> Self {
        Self {
            sector_size: DEFAULT_SECTOR_SIZE,
            validate_crc: true,
            validate_bounds: true,
        }
    }
}

impl DiskScanOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pratique pour désactiver rapidement les checks lourds
    pub fn no_crc(mut self) -> Self {
        self.validate_crc = false;
        self
    }

    pub fn no_bounds(mut self) -> Self {
        self.validate_bounds = false;
        self
    }

    pub fn with_sector_size(mut self, sz: u64) -> Self {
        self.sector_size = sz;
        self
    }
}

/// Infos d’une partition
#[cfg(feature = "alloc")]
#[derive(Debug, Clone)]
pub struct PartitionInfo {
    pub index: usize,
    pub kind: GptPartitionKind,
    pub unique_guid: [u8; 16],
    pub start_lba: u64,
    pub end_lba: u64,
    pub start_bytes: u64,
    pub size_bytes: u64,
    pub attrs: u64,
    pub name: String,
}

/// Résultat global du scan
#[cfg(feature = "alloc")]
#[derive(Debug, Clone)]
pub struct DiskInfo {
    pub mbr_kind: MbrKind,
    pub sector_size: u64,
    pub gpt_header: Option<GptHeader>,
    pub partitions: Vec<PartitionInfo>,
}

impl core::fmt::Display for DiskInfo {
    #[cfg(all(not(feature = "std"), feature = "alloc"))]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(
            f,
            "Disk Layout: sector_size={}  MBR={:?}  GPT={}",
            self.sector_size,
            self.mbr_kind,
            if self.gpt_header.is_some() {
                "present"
            } else {
                "absent"
            }
        )?;
        for p in &self.partitions {
            writeln!(
                f,
                "  + part[{}] name={} type={} lba={}..{} size={}",
                p.index, p.name, p.kind, p.start_lba, p.end_lba, p.size_bytes
            )?;
        }
        f
    }

    #[cfg(feature = "std")]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let gpt_state = if self.gpt_header.is_some() {
            "present"
        } else {
            "absent"
        };

        writeln!(
            f,
            "Disk layout • sector: {} • MBR: {:?} • GPT: {}",
            sep_u64(self.sector_size),
            self.mbr_kind,
            gpt_state
        )?;

        writeln!(
            f,
            "  ┌────┬──────────────────────────────┬──────────────────────────────┬────────────┬────────────┬───────────────┐"
        )?;
        writeln!(
            f,
            "  | Id | Name                         | Type                         | Start LBA  | End LBA    | Size          |"
        )?;
        writeln!(
            f,
            "  ├────┼──────────────────────────────┼──────────────────────────────┼────────────┼────────────┼───────────────┤"
        )?;

        for p in &self.partitions {
            writeln!(
                f,
                "  | {:<2} | {:<28} | {:<28} | {:>10} | {:>10} | {:>13} |",
                p.index,
                truncate(&p.name, 28),
                truncate(&p.kind.to_string(), 28),
                sep_u64(p.start_lba),
                sep_u64(p.end_lba),
                pretty_bytes(p.size_bytes),
            )?;
        }

        writeln!(
            f,
            "  └────┴──────────────────────────────┴──────────────────────────────┴────────────┴────────────┴───────────────┘"
        )
    }
}

/// Décode un nom GPT (UTF-16LE, 36 * u16) en String rust.
/// Stoppe au premier 0, remplace les code points invalides.
#[cfg(feature = "alloc")]
fn decode_gpt_name(name: &[u16; 36]) -> String {
    let end = name.iter().position(|&c| c == 0).unwrap_or(36);
    String::from_utf16_lossy(&name[..end])
}

/// Scan principal : détecte MBR (vide/protectif/legacy), GPT et partitions
#[cfg(feature = "alloc")]
pub fn scan_disk<IO: BlockIO + ?Sized>(io: &mut IO, opts: DiskScanOptions) -> PartResult<DiskInfo> {
    // 1) Lire l’MBR brut (on n’échoue pas si non-protectif)
    let mbr: Mbr = io.read_struct(0)?;
    let mbr_kind = {
        if mbr.signature != mbr::MBR_SIGNATURE {
            MbrKind::Empty
        } else if mbr.entries.iter().any(|e| e.part_type == PROTECTIVE_GPT) {
            MbrKind::Protective
        } else if mbr.entries.iter().any(|e| e.part_type != 0) {
            MbrKind::Legacy
        } else {
            MbrKind::Empty
        }
    };

    // 2) S’il y a GPT, lire header+entries (avec ou sans validations)
    let mut gpt_header: Option<GptHeader> = None;
    let mut parts: Vec<PartitionInfo> = Vec::new();

    if matches!(mbr_kind, MbrKind::Protective) {
        let (header, entries) = gpt::read_gpt_with_sector(io, opts.sector_size)?;

        header.validate_header()?;
        if opts.validate_crc {
            header.validate_crc(&entries)?;
        }
        if opts.validate_bounds {
            header.validate_entries(&entries, opts.sector_size)?;
        }

        gpt_header = Some(header);

        for (idx, e) in entries.iter().enumerate() {
            let start_lba = e.start_lba;
            let end_lba = e.end_lba;
            let start_bytes = start_lba
                .checked_mul(opts.sector_size)
                .ok_or(PartError::Other("start_bytes overflow"))?;
            let size_lba = end_lba
                .checked_sub(start_lba)
                .and_then(|n| n.checked_add(1))
                .ok_or(PartError::Other("size_lba underflow"))?;
            let size_bytes = size_lba
                .checked_mul(opts.sector_size)
                .ok_or(PartError::Other("size_bytes overflow"))?;

            let name = e.name;

            parts.push(PartitionInfo {
                index: idx,
                kind: GptPartitionKind::from_guid(&e.type_guid),
                unique_guid: e.unique_guid,
                start_lba,
                end_lba,
                start_bytes,
                size_bytes,
                attrs: e.attributes,
                name: decode_gpt_name(&name),
            });
        }
    }

    Ok(DiskInfo {
        mbr_kind,
        sector_size: opts.sector_size,
        gpt_header,
        partitions: parts,
    })
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    &s[..max]
}

#[cfg(all(feature = "std", feature = "alloc"))]
fn pretty_bytes(n: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut val = n as f64;
    let mut idx = 0usize;
    while val >= 1024.0 && idx + 1 < UNITS.len() {
        val /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", sep_u64(n), UNITS[idx])
    } else {
        format!("{:.1} {}", val, UNITS[idx])
    }
}

#[cfg(all(feature = "std", feature = "alloc"))]
fn sep_u64(mut n: u64) -> String {
    // séparateur de milliers « fine »: 12 345 678
    if n < 1_000 {
        return n.to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    while n >= 1_000 {
        parts.push(format!("{:03}", (n % 1_000)));
        n /= 1_000;
    }
    parts.push(n.to_string());
    parts.reverse();
    parts.join(" ") // espace fine insécable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gpt, guids, mbr};

    #[test]
    fn scan_protective_gpt_happy_path() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = rimio::prelude::MemBlockIO::new(&mut buf);

        // 1) MBR protectif
        mbr::write_mbr_protective(&mut io, 20_000).unwrap();

        // 2) GPT avec 2 partitions
        let p1 = GptEntry::new(
            guids::GPT_PARTITION_TYPE_ESP,
            [1; 16],
            2048,
            4095,
            0,
            "EFI-SYSTEM",
        );
        let p2 = GptEntry::new(
            guids::GPT_PARTITION_TYPE_LINUX,
            [2; 16],
            4096,
            10_000,
            0,
            "rootfs",
        );
        gpt::write_gpt(&mut io, &[p1, p2], 20_000, [0xAB; 16]).unwrap();

        // 3) Scan
        let info = scan_disk(&mut io, DiskScanOptions::default()).unwrap();

        assert!(matches!(info.mbr_kind, MbrKind::Protective));
        assert_eq!(info.partitions.len(), 2);
        assert_eq!(info.partitions[0].name, "EFI-SYSTEM");
        assert_eq!(
            format!("{}", info.partitions[0].kind),
            "EFI System Partition"
        );
        assert_eq!(info.partitions[0].start_lba, 2048);
        assert_eq!(info.partitions[1].name, "rootfs");

        // 4) Display (smoke)
        println!("{info}");
    }

    #[test]
    fn scan_without_crc_validation() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = rimio::prelude::MemBlockIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, 20_000).unwrap();

        // GPT minimal : une partition
        let p = GptEntry::new(
            guids::GPT_PARTITION_TYPE_DATA,
            [9; 16],
            2048,
            3071,
            0,
            "data",
        );
        gpt::write_gpt(&mut io, &[p], 20_000, [0xAB; 16]).unwrap();

        // Scan rapide sans CRC
        let info = scan_disk(&mut io, DiskScanOptions::new().no_crc()).unwrap();
        assert_eq!(info.partitions.len(), 1);
        assert_eq!(info.partitions[0].name, "data");
    }
}
