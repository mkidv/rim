// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use crate::errors::*;
use rimio::prelude::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const MBR_SIGNATURE: [u8; 2] = [0x55, 0xAA];
pub const PROTECTIVE_GPT: u8 = 0xEE;

// ---------- Helpers "legacy" ----------
#[inline]
fn lba_end_inclusive(start_lba: u32, sectors: u32) -> PartResult<u64> {
    if sectors == 0 {
        return Err(MbrError::ZeroSectors.into());
    }
    // end = start + sectors - 1 (as u64 to avoid overflow)
    let end = (start_lba as u64)
        .checked_add(sectors as u64 - 1)
        .ok_or(PartError::Other("MBR: LBA range overflow"))?;
    Ok(end)
}

#[allow(dead_code)]
#[inline]
fn is_extended_type(t: u8) -> bool {
    // Classic extended types (CHS/LBA); includes 0x85 (Linux extended)
    matches!(t, 0x05 | 0x0F | 0x85)
}

#[allow(dead_code)]
#[inline]
fn is_protective_type(t: u8) -> bool {
    t == PROTECTIVE_GPT
}

#[inline]
fn is_known_legacy_type(t: u8) -> bool {
    // Reasonable whitelist (not exhaustive, but "mainstream")
    matches!(
        t,
        0x01 | 0x04 | 0x06            // FAT12/16
        | 0x07                        // NTFS/exFAT/HPFS (NT)
        | 0x0B | 0x0C | 0x0E          // FAT32/FAT32 LBA/FAT16 LBA
        | 0x82 | 0x83 | 0x8E          // Linux swap / Linux / LVM
        | 0xA5 | 0xA6 | 0xA8 | 0xAB   // BSD / NetBSD / Darwin UFS / Apple boot
        | 0xAF | 0xFB | 0xFD          // Apple HFS / VMware / Linux RAID
        | 0xEE                        // protective GPT (non-legacy, but "known")
        | 0x05 | 0x0F | 0x85 // extended
    )
}

#[cfg(feature = "alloc")]
fn check_overlaps_legacy(entries: &[MbrEntry]) -> PartResult<()> {
    let mut segs: Vec<(u64, u64)> = Vec::with_capacity(entries.len());
    for e in entries.iter().filter(|e| !e.is_empty()) {
        let end = lba_end_inclusive(e.start_lba, e.sectors)?;
        segs.push((e.start_lba as u64, end));
    }
    if segs.len() <= 1 {
        return Ok(());
    }
    segs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let mut prev = segs[0];
    for &curr in &segs[1..] {
        if curr.0 <= prev.1 {
            return Err(MbrError::Overlap {
                a_start: prev.0,
                a_end: prev.1,
                b_start: curr.0,
                b_end: curr.1,
            }
            .into());
        }
        prev = curr;
    }
    Ok(())
}

#[cfg(all(not(feature = "alloc"), not(feature = "std")))]
fn check_overlaps_legacy(entries: &[MbrEntry]) -> PartResult<()> {
    let n = entries.len();
    for i in 0..n {
        let a = &entries[i];
        if a.is_empty() {
            continue;
        }
        let a_end = lba_end_inclusive(a.start_lba, a.sectors)?;
        for j in (i + 1)..n {
            let b = &entries[j];
            if b.is_empty() {
                continue;
            }
            let b_end = lba_end_inclusive(b.start_lba, b.sectors)?;
            // inclusive: overlap iff a.start <= b.end && b.start <= a.end
            if (a.start_lba as u64) <= b_end && (b.start_lba as u64) <= a_end {
                return Err(MbrError::Overlap {
                    a_start: a.start_lba as u64,
                    a_end,
                    b_start: b.start_lba as u64,
                    b_end,
                }
                .into());
            }
        }
    }
    Ok(())
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)] // 16 bytes, correctly aligned
pub struct MbrEntry {
    pub boot_flag: u8,
    pub starting_chs: [u8; 3],
    pub part_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,
    pub sectors: u32,
}

impl MbrEntry {
    #[inline]
    pub fn new(
        boot_flag: u8,
        starting_chs: [u8; 3],
        part_type: u8,
        end_chs: [u8; 3],
        start_lba: u32,
        sectors: u32,
    ) -> Self {
        Self {
            boot_flag,
            starting_chs,
            part_type,
            end_chs,
            start_lba,
            sectors,
        }
    }

    #[inline]
    pub fn new_empty() -> Self {
        Self::new(0x00, [0, 0, 0], 0x00, [0, 0, 0], 0, 0)
    }

    #[inline]
    pub fn new_protective(total_sectors: u64) -> Self {
        let sectors = total_sectors.saturating_sub(1);
        Self::new(
            0x00,
            [0x00, 0x02, 0x00],
            PROTECTIVE_GPT,
            [0xFE, 0xFF, 0xFF],
            1,
            if sectors > u32::MAX as u64 {
                u32::MAX
            } else {
                sectors as u32
            },
        )
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.part_type == 0
    }

    #[inline]
    pub fn is_protective(&self) -> bool {
        self.part_type == PROTECTIVE_GPT
    }

    #[inline]
    pub fn validate_basic(&self) -> PartResult<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.sectors == 0 {
            return Err(MbrError::ZeroSectors.into());
        }
        if !(self.boot_flag == 0x00 || self.boot_flag == 0x80) {
            return Err(MbrError::InvalidBootFlag {
                got: self.boot_flag,
            }
            .into());
        }
        Ok(())
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct MbrEntryPacked {
    pub boot_flag: u8,
    pub starting_chs: [u8; 3],
    pub part_type: u8,
    pub end_chs: [u8; 3],
    pub start_lba: u32,
    pub sectors: u32,
}

impl MbrEntryPacked {
    #[inline]
    pub fn to_aligned(self) -> MbrEntry {
        MbrEntry {
            boot_flag: self.boot_flag,
            starting_chs: self.starting_chs,
            part_type: self.part_type,
            end_chs: self.end_chs,
            start_lba: u32::from_le(self.start_lba),
            sectors: u32::from_le(self.sectors),
        }
    }

    #[inline]
    pub fn from_aligned(e: &MbrEntry) -> Self {
        Self {
            boot_flag: e.boot_flag,
            starting_chs: e.starting_chs,
            part_type: e.part_type,
            end_chs: e.end_chs,
            start_lba: e.start_lba.to_le(),
            sectors: e.sectors.to_le(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MbrKind {
    Empty,
    Protective,
    Legacy,
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct Mbr {
    pub boot_code: [u8; 446],
    pub entries: [MbrEntryPacked; 4],
    pub signature: [u8; 2],
}

impl Mbr {
    #[inline]
    pub fn new_from_entries(entries: [MbrEntry; 4]) -> Self {
        let packed = entries.map(|e| MbrEntryPacked::from_aligned(&e));
        Self {
            boot_code: [0u8; 446],
            entries: packed,
            signature: MBR_SIGNATURE,
        }
    }

    #[inline]
    pub fn new_empty() -> Self {
        Self::new_from_entries([MbrEntry::new_empty(); 4])
    }

    #[inline]
    pub fn new_protective(total_sectors: u64) -> Self {
        let mut es = [MbrEntry::new_empty(); 4];
        es[0] = MbrEntry::new_protective(total_sectors);
        Self::new_from_entries(es)
    }

    #[inline]
    pub fn has_valid_signature(&self) -> bool {
        self.signature == MBR_SIGNATURE
    }

    #[inline]
    pub fn aligned_entries(&self) -> [MbrEntry; 4] {
        [
            self.entries[0].to_aligned(),
            self.entries[1].to_aligned(),
            self.entries[2].to_aligned(),
            self.entries[3].to_aligned(),
        ]
    }

    #[inline]
    pub fn first_non_empty(&self) -> Option<MbrEntry> {
        self.aligned_entries().into_iter().find(|e| !e.is_empty())
    }

    pub fn kind(&self) -> MbrKind {
        let Some(first) = self.first_non_empty() else {
            return MbrKind::Empty;
        };
        if first.is_protective() {
            MbrKind::Protective
        } else {
            MbrKind::Legacy
        }
    }

    #[inline]
    pub fn validate_header(&self) -> PartResult<()> {
        if !self.has_valid_signature() {
            return Err(MbrError::InvalidSignature {
                expected: MBR_SIGNATURE,
                found: self.signature,
            }
            .into());
        }
        Ok(())
    }

    #[inline]
    pub fn validate_entries_basic(&self) -> PartResult<()> {
        for e in self.aligned_entries().iter() {
            e.validate_basic()?;
        }
        Ok(())
    }

    #[inline]
    pub fn validate_protective(&self, total_sectors: u64) -> PartResult<()> {
        self.validate_header()?;
        self.validate_entries_basic()?;

        let Some(first) = self.first_non_empty() else {
            return Err(MbrError::ProtectiveMissing.into());
        };
        if !first.is_protective() {
            return Err(MbrError::ProtectiveMissing.into());
        }
        let es = self.aligned_entries();
        if es.iter().skip(1).any(|e| !e.is_empty()) {
            return Err(MbrError::ProtectiveExtraEntries.into());
        }

        if total_sectors > 0 {
            let expected = total_sectors.saturating_sub(1);
            if total_sectors > u32::MAX as u64 {
                if first.sectors != u32::MAX {
                    return Err(MbrError::ProtectiveSizeMismatch {
                        expected: u32::MAX,
                        got: first.sectors,
                        gt_2tib: true,
                    }
                    .into());
                }
            } else if first.sectors != expected as u32 {
                return Err(MbrError::ProtectiveSizeMismatch {
                    expected: expected as u32,
                    got: first.sectors,
                    gt_2tib: false,
                }
                .into());
            }
        }

        Ok(())
    }

    #[inline]
    pub fn validate_legacy(&self) -> PartResult<()> {
        self.validate_header()?;
        self.validate_entries_basic()?;
        let es = self.aligned_entries();
        // No 0xEE type here (otherwise it's protective)
        if es.iter().any(|e| e.is_protective()) {
            return Err(PartError::Other(
                "MBR: protective entry present in legacy MBR",
            ));
        }
        for e in es.iter().filter(|e| !e.is_empty()) {
            if !is_known_legacy_type(e.part_type) {
                return Err(MbrError::UnsupportedType { ty: e.part_type }.into());
            }
        }
        check_overlaps_legacy(&es)?;
        Ok(())
    }
}

pub fn write_mbr<IO: RimIO + ?Sized>(io: &mut IO, mbr: &Mbr) -> PartResult<()> {
    io.write_struct(0, mbr)?;
    io.flush()?;
    Ok(())
}

pub fn write_mbr_protective<IO: RimIO + ?Sized>(io: &mut IO, total_sectors: u64) -> PartResult<()> {
    let mbr = Mbr::new_protective(total_sectors);
    write_mbr(io, &mbr)
}

pub fn write_mbr_from_entries<IO: RimIO + ?Sized>(
    io: &mut IO,
    entries: [MbrEntry; 4],
) -> PartResult<()> {
    let mbr = Mbr::new_from_entries(entries);
    write_mbr(io, &mbr)
}

pub fn read_mbr<IO: RimIO + ?Sized>(io: &mut IO) -> PartResult<Mbr> {
    let mbr: Mbr = io.read_struct(0)?;
    mbr.validate_header()?;
    Ok(mbr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_parse_protective_mbr() {
        let mut buf = [0u8; 512];
        let mut io = MemRimIO::new(&mut buf);

        write_mbr_protective(&mut io, 2048).unwrap();
        let mbr = read_mbr(&mut io).unwrap();

        assert_eq!(mbr.signature, MBR_SIGNATURE);
        let entries = mbr.aligned_entries();
        assert_eq!(entries[0].part_type, PROTECTIVE_GPT);
    }

    #[test]
    fn validate_mbr_invalid_signature() {
        let bad = Mbr {
            boot_code: [0; 446],
            entries: [MbrEntryPacked::from_aligned(&MbrEntry::new_protective(1)); 4],
            signature: [0x00, 0x00],
        };
        assert!(&bad.validate_header().is_err());
        assert!(&bad.validate_protective(1).is_err());
    }

    #[test]
    fn mbr_kind_empty() {
        let mbr = Mbr::new_empty();
        assert_eq!(mbr.kind(), MbrKind::Empty);
    }

    #[test]
    fn mbr_kind_protective() {
        let mbr = Mbr::new_protective(2048);
        assert_eq!(mbr.kind(), MbrKind::Protective);
    }

    #[test]
    fn mbr_kind_legacy() {
        let legacy = MbrEntry::new(
            0x80, // bootable
            [0x00, 0x02, 0x00],
            0x83, // Linux
            [0xFE, 0xFF, 0xFF],
            2048,
            4096,
        );
        let mut aligned = [MbrEntry::new_empty(); 4];
        aligned[0] = legacy;
        let mbr = Mbr::new_from_entries(aligned);

        assert_eq!(mbr.kind(), MbrKind::Legacy);
        let e0 = mbr.first_non_empty().unwrap();
        assert_eq!(e0.part_type, 0x83);
        assert_eq!(e0.boot_flag, 0x80);
    }
}
