// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::guids::GptPartitionKind;
use crate::{errors::*, io_ext::BlockIOLbaExt};
use rimio::prelude::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const GPT_ENTRY_SIZE: usize = 128;
pub const GPT_PRIMARY_ENTRIES_LBA: u64 = 2;
pub const GPT_PRIMARY_HEADER_LBA: u64 = 1;
pub const GPT_DEFAULT_NUM_ENTRIES: usize = 128;
pub const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";
pub const GPT_REVISION: u32 = 0x00010000;

#[inline]
pub fn align_up(v: u64, a: u64) -> u64 {
    if v % a == 0 { v } else { v + (a - (v % a)) }
}
#[inline]
pub fn align_down(v: u64, a: u64) -> u64 {
    v - (v % a)
}

#[inline]
pub fn align_lba_1m(sector_size: u64) -> u64 {
    ((1 << 20) / sector_size).max(1)
}

pub fn encode_gpt_name(name: &str) -> [u16; 36] {
    let mut buf = [0u16; 36];
    for (i, c) in name.encode_utf16().take(36).enumerate() {
        buf[i] = c;
    }
    buf
}

#[inline]
fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

#[inline]
fn compute_header_crc32(mut header: GptHeader) -> u32 {
    header.header_crc = 0;
    let bytes = header.as_bytes();
    crc32(&bytes[..header.header_size as usize])
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C)]
pub struct GptEntry {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub start_lba: u64,
    pub end_lba: u64,
    pub attributes: u64,
    pub name: [u16; 36],
}

impl GptEntry {
    pub fn new(
        type_guid: [u8; 16],
        unique_guid: [u8; 16],
        start_lba: u64,
        end_lba: u64,
        attributes: u64,
        name: &str,
    ) -> Self {
        Self {
            type_guid,
            unique_guid,
            start_lba,
            end_lba,
            attributes,
            name: encode_gpt_name(name),
        }
    }

    #[inline]
    pub fn kind(&self) -> GptPartitionKind {
        GptPartitionKind::from_guid(&self.type_guid)
    }

    #[inline]
    pub fn is_known_kind(&self) -> bool {
        !matches!(self.kind(), GptPartitionKind::Unknown(_))
    }

    pub fn is_empty(&self) -> bool {
        self.type_guid.iter().all(|&b| b == 0)
            && self.unique_guid.iter().all(|&b| b == 0)
            && self.start_lba == 0
            && self.end_lba == 0
            && self.attributes == 0
            && self.name.iter().all(|&c| c == 0)
    }

    pub fn validate_basic(&self) -> PartResult<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.end_lba < self.start_lba {
            return Err(PartError::Other("GPT: partition ends before it starts"));
        }
        Ok(())
    }

    pub fn validate_in_bounds(
        &self,
        first_usable: u64,
        last_usable: u64,
        align: u64,
    ) -> PartResult<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.start_lba < first_usable {
            return Err(PartError::Other(
                "GPT: partition starts before first usable LBA",
            ));
        }
        if self.end_lba > last_usable {
            return Err(PartError::Other(
                "GPT: partition ends after last usable LBA",
            ));
        }
        if self.start_lba % align != 0 {
            return Err(PartError::Other("GPT: partition start not aligned"));
        }
        Ok(())
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C)]
pub struct GptHeader {
    pub signature: [u8; 8],
    pub revision: u32,
    pub header_size: u32,
    pub header_crc: u32,
    pub reserved: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub entries_lba: u64,
    pub num_entries: u32,
    pub entry_size: u32,
    pub entries_crc: u32,
    pub reserved2: [u8; 420],
}

impl GptHeader {
    pub fn new_primary(
        total_sectors: u64,
        disk_guid: [u8; 16],
        num_entries: u32,
        entry_size: u32,
        sector_size: u64,
    ) -> PartResult<Self> {
        let entries_sectors = (num_entries as u64 * entry_size as u64).div_ceil(sector_size);
        let align = align_lba_1m(sector_size);

        let entries_lba = GPT_PRIMARY_ENTRIES_LBA;
        let raw_first = entries_lba + entries_sectors;
        let tail = 1 + entries_sectors;

        let raw_last = total_sectors
            .checked_sub(1)
            .and_then(|x| x.checked_sub(tail))
            .ok_or(PartError::Other("GPT: disk too small (headers/tables)"))?;

        let first_usable_lba = align_up(raw_first, align);
        let last_usable_lba = align_down(raw_last, align);

        if first_usable_lba > last_usable_lba {
            return Err(PartError::Other("GPT: disk too small for 1MiB alignment"));
        }

        Ok(Self {
            signature: *GPT_SIGNATURE,
            revision: GPT_REVISION,
            header_size: 92,
            header_crc: 0,
            reserved: 0,
            current_lba: GPT_PRIMARY_HEADER_LBA,
            backup_lba: total_sectors - 1,
            first_usable_lba,
            last_usable_lba,
            disk_guid,
            entries_lba,
            num_entries,
            entry_size,
            entries_crc: 0,
            reserved2: [0u8; 420],
        })
    }

    pub fn to_backup(mut self, total_sectors: u64, backup_entries_lba: u64) -> Self {
        self.current_lba = total_sectors - 1;
        self.backup_lba = GPT_PRIMARY_HEADER_LBA;
        self.entries_lba = backup_entries_lba;
        self.header_crc = compute_header_crc32(self);
        self
    }

    pub fn compute_crc32(&mut self, entries: &[GptEntry]) {
        self.entries_crc =
            compute_entries_crc32(entries, self.num_entries as usize, self.entry_size as usize);
        self.header_crc = compute_header_crc32(*self);
    }

    pub fn validate_header(&self) -> PartResult<()> {
        if &self.signature != GPT_SIGNATURE {
            return Err(PartError::Invalid("GPT: invalid signature"));
        }
        if self.revision != GPT_REVISION {
            return Err(PartError::Invalid("GPT: unsupported revision"));
        }
        if self.header_size < 92 {
            return Err(PartError::Invalid("GPT: header_size too small"));
        }
        let base = core::mem::size_of::<GptEntry>() as u32;
        if self.entry_size < base || (self.entry_size % 8) != 0 {
            return Err(PartError::Invalid("GPT: invalid entry_size"));
        }
        Ok(())
    }

    pub fn validate_crc(&self, entries: &[GptEntry]) -> PartResult<()> {
        if compute_header_crc32(*self) != self.header_crc {
            return Err(PartError::Invalid("GPT: header CRC mismatch"));
        }

        if compute_entries_crc32(entries, self.num_entries as usize, self.entry_size as usize)
            != self.entries_crc
        {
            return Err(PartError::Invalid("GPT: entries CRC mismatch"));
        }
        Ok(())
    }

    pub fn validate_entries(&self, entries: &[GptEntry], sector_size: u64) -> PartResult<()> {
        let align = align_lba_1m(sector_size);

        for e in entries {
            e.validate_basic()?;
            e.validate_in_bounds(self.first_usable_lba, self.last_usable_lba, align)?;
        }

        check_overlaps(entries)?;

        Ok(())
    }
}

#[inline]
fn overlaps_inclusive(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start <= b_end && b_start <= a_end
}

#[cfg(any(feature = "alloc", feature = "std"))]
fn check_overlaps(entries: &[GptEntry]) -> PartResult<()> {
    let mut segs: Vec<(u64, u64)> = entries
        .iter()
        .filter(|e| !e.is_empty())
        .map(|e| (e.start_lba, e.end_lba))
        .collect();

    if segs.len() <= 1 {
        return Ok(());
    }

    segs.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let mut prev = segs[0];
    for &curr in &segs[1..] {
        if overlaps_inclusive(prev.0, prev.1, curr.0, curr.1) {
            return Err(PartError::Other("GPT: partition overlap detected"));
        }
        // avancer la “frontière”
        prev = curr;
    }
    Ok(())
}

#[cfg(all(not(feature = "alloc"), not(feature = "std")))]
fn check_overlaps(entries: &[GptEntry]) -> PartResult<()> {
    // Fallback O(n²) sans allocation
    let n = entries.len();
    for i in 0..n {
        let a = &entries[i];
        if a.is_empty() {
            continue;
        }
        for j in (i + 1)..n {
            let b = &entries[j];
            if b.is_empty() {
                continue;
            }
            if overlaps_inclusive(a.start_lba, a.end_lba, b.start_lba, b.end_lba) {
                return Err(PartError::Other("GPT: partition overlap detected"));
            }
        }
    }
    Ok(())
}

fn compute_entries_crc32(entries: &[GptEntry], num_entries: usize, entry_size: usize) -> u32 {
    let base = core::mem::size_of::<GptEntry>();
    let mut hasher = crc32fast::Hasher::new();
    let mut slot = vec![0u8; entry_size]; // petite alloc réutilisée
    for i in 0..num_entries {
        slot.fill(0);
        if let Some(e) = entries.get(i) {
            let b = e.as_bytes();
            slot[..base].copy_from_slice(b);
        }
        hasher.update(&slot);
    }
    hasher.finalize()
}

fn write_entries<IO: BlockIO + ?Sized>(
    io: &mut IO,
    entries_lba: u64,
    sector_size: u64,
    entries: &[GptEntry],
    num_entries: usize,
    entry_size: usize,
) -> PartResult<()> {
    let base = core::mem::size_of::<GptEntry>();
    let per_sector = (sector_size as usize) / entry_size;
    if per_sector == 0 {
        return Err(PartError::Invalid("GPT: entry size exceeds sector size"));
    }
    let mut sector = vec![0u8; sector_size as usize];

    let mut idx = 0usize;
    let mut lba = entries_lba;
    while idx < num_entries {
        sector.fill(0);
        let take = core::cmp::min(per_sector, num_entries - idx);
        for s in 0..take {
            let dst = &mut sector[s * entry_size..(s + 1) * entry_size];
            if let Some(e) = entries.get(idx + s) {
                let b = e.as_bytes();
                dst[..base].copy_from_slice(b);
            }
        }
        io.write_at_lba(lba, sector_size, &sector)?;
        lba += 1;
        idx += take;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Write GPT
// ---------------------------------------------------------------------

pub fn write_gpt_with_sector<IO: BlockIO + ?Sized>(
    io: &mut IO,
    entries: &[GptEntry],
    total_sectors: u64,
    disk_guid: [u8; 16],
    sector_size: u64,
) -> PartResult<()> {
    let num_entries = GPT_DEFAULT_NUM_ENTRIES as u32;
    let entry_size = GPT_ENTRY_SIZE as u32;

    let mut primary = GptHeader::new_primary(
        total_sectors,
        disk_guid,
        num_entries,
        entry_size,
        sector_size,
    )?;

    primary.compute_crc32(entries);

    write_entries(
        io,
        primary.entries_lba,
        sector_size,
        entries,
        primary.num_entries as usize,
        primary.entry_size as usize,
    )?;
    io.write_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size, &primary)?;

    let entries_sectors =
        (primary.num_entries as u64 * primary.entry_size as u64).div_ceil(sector_size);
    let backup_entries_lba = (total_sectors - 1) - entries_sectors;

    let secondary = primary.to_backup(total_sectors, backup_entries_lba);

    write_entries(
        io,
        backup_entries_lba,
        sector_size,
        entries,
        secondary.num_entries as usize,
        secondary.entry_size as usize,
    )?;
    io.write_struct_lba(total_sectors - 1, sector_size, &secondary)?;

    io.flush()?;
    Ok(())
}

pub fn write_gpt<IO: BlockIO + ?Sized>(
    io: &mut IO,
    entries: &[GptEntry],
    total_sectors: u64,
    disk_guid: [u8; 16],
) -> PartResult<()> {
    write_gpt_with_sector(
        io,
        entries,
        total_sectors,
        disk_guid,
        crate::DEFAULT_SECTOR_SIZE,
    )
}

fn read_entries(buf: &[u8], entry_size: usize) -> PartResult<Vec<GptEntry>> {
    let base = core::mem::size_of::<GptEntry>();
    if entry_size < base || (entry_size % 8) != 0 {
        return Err(PartError::Invalid("GPT: Invalid entry size"));
    }

    let count = buf.len() / entry_size;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let slot = &buf[i * entry_size..(i + 1) * entry_size];
        let head = &slot[..base];
        let e = GptEntry::ref_from_bytes(head)
            .map_err(|_| PartError::Invalid("GPT: Invalid entry"))?;
        if !e.is_empty() {
            out.push(*e);
        }
    }
    Ok(out)
}

pub fn read_gpt_with_sector<IO: BlockIO + ?Sized>(
    io: &mut IO,
    sector_size: u64,
) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    let hdr: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;

    hdr.validate_header()?;

    let es = hdr.entry_size as usize;
    let ne = hdr.num_entries as usize;
    if es > 512 {
        return Err(PartError::Invalid("GPT: entry_size too large"));
    }
    if ne == 0 || ne > 16_384 {
        return Err(PartError::Invalid("GPT: num_entries out of range"));
    }

    let buf_len = ne
        .checked_mul(es)
        .ok_or(PartError::Invalid("GPT: entries byte length overflow"))?;
    let mut buf = vec![0u8; buf_len];
    io.read_at_lba(hdr.entries_lba, sector_size, &mut buf)?;

    let entries = read_entries(&buf, hdr.entry_size as usize)?;

    Ok((hdr, entries))
}

pub fn read_gpt<IO: BlockIO + ?Sized>(io: &mut IO) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    read_gpt_with_sector(io, crate::DEFAULT_SECTOR_SIZE)
}

/// Place des entrées à la suite, alignées 1 MiB, dans les bornes du header.
/// Retourne une Vec<GptEntry> ou une erreur si ça ne rentre pas.
/// Conçu pour tests et cas simples (pas d’intervalles imposés).
#[cfg(feature = "alloc")]
pub fn make_aligned_entries<'a, I>(
    header: &GptHeader,
    sector_size: u64,
    reqs: I,
) -> PartResult<Vec<GptEntry>>
where
    I: IntoIterator<
        Item = (
            &'a [u8; 16],
            &'a [u8; 16],
            u64, /*len_sectors*/
            u64, /*attrs*/
            &'a str,
        ),
    >,
{
    let align = align_lba_1m(sector_size);
    let mut cur = header.first_usable_lba;
    let mut out = vec![];

    for (typ, uid, len_sectors, attrs, name) in reqs {
        if len_sectors == 0 {
            return Err(PartError::Other("GPT: zero-sized allocation"));
        }
        // aligner le début
        if cur % align != 0 {
            cur += align - (cur % align);
        }
        let end = cur
            .checked_add(len_sectors - 1)
            .ok_or(PartError::Invalid("GPT: LBA overflow"))?;
        if end > header.last_usable_lba {
            return Err(PartError::Other("GPT: not enough usable space"));
        }
        out.push(GptEntry::new(*typ, *uid, cur, end, attrs, name));
        cur = end.saturating_add(1);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use crate::DEFAULT_SECTOR_SIZE;

    use super::*;

    #[test]
    fn write_and_parse_gpt() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = MemBlockIO::new(&mut buf);

        let part = GptEntry::new([1; 16], [2; 16], 2048, 4095, 0, "test");

        write_gpt(&mut io, &[part], 20_000, [0xAB; 16]).unwrap();

        let (header, parts) = read_gpt(&mut io).unwrap();
        assert_eq!(header.signature, *GPT_SIGNATURE);
        assert_eq!(header.disk_guid, [0xAB; 16]);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].start_lba, 2048);
    }

    #[test]
    fn overlap_detection() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = MemBlockIO::new(&mut buf);

        let p1 = GptEntry::new([1; 16], [2; 16], 2048, 4095, 0, "1");
        let p2 = GptEntry::new([3; 16], [4; 16], 3000, 5000, 0, "2");
        write_gpt(&mut io, &[p1, p2], 20_000, [0xAB; 16]).unwrap();
        let (header, parts) = read_gpt(&mut io).unwrap();
        assert!(
            header
                .validate_entries(&parts, DEFAULT_SECTOR_SIZE)
                .is_err()
        );
    }
}
