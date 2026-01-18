// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};

use rimio::prelude::*;

use crate::{
    DEFAULT_SECTOR_SIZE,
    errors::*,
    gpt::{self, GptHeader},
    guids::GptPartitionKind,
    mbr::{self, Mbr, MbrKind, PROTECTIVE_GPT},
};

/// Partition information
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

/// Global scan result
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
        Ok(())
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

/// Main scan: detects MBR (empty/protective/legacy), GPT, and partitions
#[cfg(feature = "alloc")]
pub fn scan_disk_with_sector<IO: RimIO + ?Sized>(
    io: &mut IO,
    sector_size: u64,
) -> PartResult<DiskInfo> {
    // Read raw MBR (don't fail if non-protective)
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

    // If GPT present, read header+entries (with or without validations)
    let mut gpt_header: Option<GptHeader> = None;
    let mut parts: Vec<PartitionInfo> = Vec::new();

    if matches!(mbr_kind, MbrKind::Protective) {
        let (header, entries) = gpt::read_gpt_with_sector(io, sector_size)?;

        gpt_header = Some(header);

        for (idx, e) in entries.iter().enumerate() {
            let start_lba = e.start_lba;
            let end_lba = e.end_lba;
            let start_bytes = start_lba
                .checked_mul(sector_size)
                .ok_or(PartError::Other("start_bytes overflow"))?;
            let size_lba = end_lba
                .checked_sub(start_lba)
                .and_then(|n| n.checked_add(1))
                .ok_or(PartError::Other("size_lba underflow"))?;
            let size_bytes = size_lba
                .checked_mul(sector_size)
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
                name: gpt::decode_gpt_name(&name),
            });
        }
    }

    Ok(DiskInfo {
        mbr_kind,
        sector_size,
        gpt_header,
        partitions: parts,
    })
}

#[cfg(feature = "alloc")]
pub fn scan_disk<IO: RimIO + ?Sized>(io: &mut IO) -> PartResult<DiskInfo> {
    scan_disk_with_sector(io, DEFAULT_SECTOR_SIZE)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    &s[..max]
}

#[cfg(feature = "alloc")]
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

#[cfg(feature = "alloc")]
fn sep_u64(mut n: u64) -> String {
    // thousands separator "fine": 12 345 678
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
    parts.join(" ") // non-breaking narrow space
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gpt::{self, GptEntry},
        guids, mbr,
    };

    #[test]
    fn scan_protective_gpt() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = rimio::prelude::MemRimIO::new(&mut buf);

        // Protective MBR
        mbr::write_mbr_protective(&mut io, 20_000).unwrap();

        // GPT with 2 partitions
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
        gpt::write_gpt_from_entries(&mut io, &[p1, p2], 20_000, [0xAB; 16]).unwrap();

        // Scan
        let info = scan_disk(&mut io).unwrap();

        assert!(matches!(info.mbr_kind, MbrKind::Protective));
        assert_eq!(info.partitions.len(), 2);
        assert_eq!(info.partitions[0].name, "EFI-SYSTEM");
        assert_eq!(
            format!("{}", info.partitions[0].kind),
            "EFI System Partition"
        );
        assert_eq!(info.partitions[0].start_lba, 2048);
        assert_eq!(info.partitions[1].name, "rootfs");

        // Display (smoke)
        println!("{info}");
    }
}
