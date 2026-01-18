// SPDX-License-Identifier: MIT
#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::vec;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use crate::DEFAULT_SECTOR_SIZE;
use crate::guids::GptPartitionKind;
use crate::{errors::*, io_ext::RimIOLbaExt};
use rimio::prelude::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const GPT_DEFAULT_ENTRY_SIZE: u32 = 128;
pub const GPT_DEFAULT_HEADER_SIZE: u32 = 92;
pub const GPT_DEFAULT_NUM_ENTRIES: u32 = 128;

pub const GPT_PRIMARY_ENTRIES_LBA: u64 = 2;
pub const GPT_PRIMARY_HEADER_LBA: u64 = 1;
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

#[cfg(feature = "alloc")]
pub fn decode_gpt_name(name: &[u16; 36]) -> String {
    let end = name.iter().position(|&c| c == 0).unwrap_or(36);
    String::from_utf16_lossy(&name[..end])
}

#[cfg(not(feature = "alloc"))]
pub fn decode_gpt_name<'a>(name: &[u16; 36], buf: &'a mut [u8]) -> Result<&'a str, PartError> {
    let mut written = 0;
    for &unit in name {
        if unit == 0 {
            break;
        }
        let ch = core::char::from_u32(unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
        written += ch.encode_utf8(&mut buf[written..]).len();
    }
    core::str::from_utf8(&buf[..written]).map_err(|_| PartError::Other("UTF-8 error"))
}

#[inline]
fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

#[inline]
fn compute_header_crc32(mut header: GptHeader) -> u32 {
    header.header_crc32 = 0;
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

    pub fn validate(&self) -> PartResult<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.start_lba > self.end_lba {
            return Err(PartError::Other("GPT: entry start > end"));
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
    pub header_crc32: u32,
    pub reserved: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub entries_lba: u64,
    pub num_entries: u32,
    pub entry_size: u32,
    pub entries_crc32: u32,
    pub reserved2: [u8; 420],
}

impl GptHeader {
    pub fn new(sector_size: u64, total_sectors: u64, disk_guid: [u8; 16]) -> PartResult<Self> {
        let entries_sectors =
            (GPT_DEFAULT_NUM_ENTRIES as u64 * GPT_DEFAULT_ENTRY_SIZE as u64).div_ceil(sector_size);
        let align = align_lba_1m(sector_size);

        let entries_lba = GPT_PRIMARY_ENTRIES_LBA;
        let raw_first = entries_lba + entries_sectors;
        let tail = 1 + entries_sectors;

        let raw_last = total_sectors
            .checked_sub(1)
            .and_then(|x| x.checked_sub(tail))
            .ok_or(GptError::DiskTooSmallForAlignment)?;

        let first_usable_lba = align_up(raw_first, align);
        let last_usable_lba = align_down(raw_last, align);

        if first_usable_lba > last_usable_lba {
            return Err(GptError::DiskTooSmallForAlignment.into());
        }

        Ok(Self {
            signature: *GPT_SIGNATURE,
            revision: GPT_REVISION,
            header_size: GPT_DEFAULT_HEADER_SIZE,
            header_crc32: 0,
            reserved: 0,
            current_lba: GPT_PRIMARY_HEADER_LBA,
            backup_lba: total_sectors - 1,
            first_usable_lba,
            last_usable_lba,
            disk_guid,
            entries_lba,
            num_entries: GPT_DEFAULT_NUM_ENTRIES,
            entry_size: GPT_DEFAULT_ENTRY_SIZE,
            entries_crc32: 0,
            reserved2: [0u8; 420],
        })
    }

    pub fn new_with_table(
        sector_size: u64,
        total_sectors: u64,
        disk_guid: [u8; 16],
        num_entries: u32,
        entry_size: u32,
    ) -> PartResult<Self> {
        // entry_size validation
        let base_es = core::mem::size_of::<crate::gpt::GptEntry>() as u32;
        if entry_size < base_es || (entry_size % 8) != 0 {
            return Err(GptError::EntrySizeInvalid {
                base: base_es,
                got: entry_size,
            }
            .into());
        }
        if entry_size > 512 {
            return Err(GptError::EntrySizeTooLarge {
                max: 512,
                got: entry_size,
            }
            .into());
        }
        if num_entries == 0 || num_entries > 16_384 {
            return Err(GptError::NumEntriesOutOfRange {
                min: 1,
                max: 16_384,
                got: num_entries,
            }
            .into());
        }

        // entry table size in sectors
        let entries_sectors = (num_entries as u64 * entry_size as u64).div_ceil(sector_size);
        let align = align_lba_1m(sector_size);

        let entries_lba = crate::gpt::GPT_PRIMARY_ENTRIES_LBA;
        let raw_first = entries_lba + entries_sectors;
        let tail = 1 + entries_sectors; // backup header (1) + backup table

        let raw_last = total_sectors
            .checked_sub(1)
            .and_then(|x| x.checked_sub(tail))
            .ok_or(GptError::DiskTooSmallForAlignment)?;

        let first_usable_lba = align_up(raw_first, align);
        let last_usable_lba = align_down(raw_last, align);

        if first_usable_lba > last_usable_lba {
            return Err(GptError::DiskTooSmallForAlignment.into());
        }

        Ok(Self {
            signature: *GPT_SIGNATURE,
            revision: GPT_REVISION,
            header_size: GPT_DEFAULT_HEADER_SIZE,
            header_crc32: 0,
            reserved: 0,
            current_lba: GPT_PRIMARY_HEADER_LBA,
            backup_lba: total_sectors - 1,
            first_usable_lba,
            last_usable_lba,
            disk_guid,
            entries_lba,
            num_entries,
            entry_size,
            entries_crc32: 0,
            reserved2: [0u8; 420],
        })
    }

    pub fn total_sectors(&self) -> u64 {
        self.backup_lba + 1
    }

    pub fn to_backup(mut self, sector_size: u64) -> Self {
        self.current_lba = self.backup_lba;
        self.backup_lba = GPT_PRIMARY_HEADER_LBA;

        let entries_sectors =
            (self.num_entries as u64 * self.entry_size as u64).div_ceil(sector_size);

        self.entries_lba = self.current_lba - entries_sectors;
        self.header_crc32 = compute_header_crc32(self);
        self
    }

    pub fn compute_crc32(&mut self, entries: &[GptEntry]) {
        self.entries_crc32 =
            compute_entries_crc32_from_iter(entries.iter().map(entry_head_bytes), self);
        self.header_crc32 = compute_header_crc32(*self);
    }

    pub fn validate_header(&self) -> PartResult {
        if &self.signature != GPT_SIGNATURE {
            return Err(GptError::InvalidSignature {
                expected: *GPT_SIGNATURE,
                found: self.signature,
            }
            .into());
        }
        if self.revision != GPT_REVISION {
            return Err(GptError::InvalidRevision {
                expected: GPT_REVISION,
                found: self.revision,
            }
            .into());
        }
        if self.header_size < GPT_DEFAULT_HEADER_SIZE {
            return Err(GptError::HeaderSizeTooSmall {
                min: GPT_DEFAULT_HEADER_SIZE,
                got: self.header_size,
            }
            .into());
        }
        let max_hdr = core::mem::size_of::<GptHeader>();
        if (self.header_size as usize) > max_hdr {
            return Err(GptError::HeaderSizeTooLarge {
                max: max_hdr,
                got: self.header_size,
            }
            .into());
        }
        if self.entry_size > 512 {
            return Err(GptError::EntrySizeTooLarge {
                max: 512,
                got: self.entry_size,
            }
            .into());
        }
        if self.num_entries == 0 || self.num_entries > 16_384 {
            return Err(GptError::NumEntriesOutOfRange {
                min: 1,
                max: 16_384,
                got: self.num_entries,
            }
            .into());
        }
        let base_es = core::mem::size_of::<GptEntry>() as u32;
        if self.entry_size < base_es || (self.entry_size % 8) != 0 {
            return Err(GptError::EntrySizeInvalid {
                base: base_es,
                got: self.entry_size,
            }
            .into());
        }
        let calc = compute_header_crc32(*self);
        if calc != self.header_crc32 {
            return Err(GptError::CrcHeaderMismatch {
                expected: self.header_crc32,
                found: calc,
            }
            .into());
        }
        Ok(())
    }

    pub fn validate_entry(&self, entry: &GptEntry, sector_size: u64) -> PartResult {
        entry.validate()?;

        let align = align_lba_1m(sector_size);
        let first_usable = self.first_usable_lba;
        let last_usable = self.last_usable_lba;

        if entry.start_lba < first_usable || entry.end_lba > last_usable {
            return Err(GptError::EntryOutOfBounds {
                first_usable,
                last_usable,
                start: entry.start_lba,
                end: entry.end_lba,
            }
            .into());
        }

        if entry.start_lba % align != 0 {
            return Err(GptError::EntryUnaligned {
                lba: entry.start_lba,
                align,
            }
            .into());
        }
        Ok(())
    }

    pub fn validate_entries(&self, entries: &[GptEntry], sector_size: u64) -> PartResult {
        let calc = compute_entries_crc32_from_iter(entries.iter().map(entry_head_bytes), self);
        if calc != self.entries_crc32 {
            return Err(GptError::CrcEntriesMismatch {
                expected: self.entries_crc32,
                found: calc,
            }
            .into());
        }

        for entry in entries {
            self.validate_entry(entry, sector_size)?
        }

        check_overlaps(entries)?;

        Ok(())
    }
}

#[inline]
pub(crate) fn overlaps_inclusive(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start <= b_end && b_start <= a_end
}

#[cfg(feature = "alloc")]
fn check_overlaps(entries: &[GptEntry]) -> PartResult {
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
            return Err(GptError::Overlap {
                a_start: prev.0,
                a_end: prev.1,
                b_start: curr.0,
                b_end: curr.1,
            }
            .into());
        }
        // advance the "boundary"
        prev = curr;
    }
    Ok(())
}

#[cfg(all(not(feature = "alloc"), not(feature = "std")))]
fn check_overlaps(entries: &[GptEntry]) -> PartResult {
    // O(n²) fallback without allocation
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
                return Err(GptError::Overlap {
                    a_start: a.start_lba,
                    a_end: a.end_lba,
                    b_start: b.start_lba,
                    b_end: b.end_lba,
                }
                .into());
            }
        }
    }
    Ok(())
}

#[inline]
fn entry_head_bytes(e: &GptEntry) -> [u8; core::mem::size_of::<GptEntry>()] {
    let mut buf = [0u8; core::mem::size_of::<GptEntry>()];
    buf.copy_from_slice(e.as_bytes());
    buf
}

#[inline]
pub fn compute_entries_crc32_from_iter<I>(mut it: I, header: &GptHeader) -> u32
where
    I: Iterator<Item = [u8; core::mem::size_of::<GptEntry>()]>,
{
    let base = core::mem::size_of::<GptEntry>();
    let entry_size = header.entry_size as usize;
    debug_assert!(entry_size >= base && entry_size % 8 == 0);

    let mut hasher = crc32fast::Hasher::new();

    let mut slot = [0u8; 512];
    let mut produced = 0usize;
    let num_entries = header.num_entries as usize;

    // Slots provided by the iterator
    for head in it.by_ref().take(num_entries) {
        slot[..base].copy_from_slice(&head);
        slot[base..entry_size].fill(0);
        hasher.update(&slot[..entry_size]);
        produced += 1;
    }

    // Padding: complete up to total_slots with zero slots
    for _ in produced..num_entries {
        // slot is already zeroed after the previous iteration,
        // but let's be explicit for readability:
        for b in &mut slot[..entry_size] {
            *b = 0;
        }
        hasher.update(&slot[..entry_size]);
    }

    hasher.finalize()
}

#[cfg(feature = "alloc")]
fn write_entries<IO: RimIO + ?Sized>(
    io: &mut IO,
    entries: &[GptEntry],
    header: &GptHeader,
    sector_size: u64,
) -> PartResult {
    let base = core::mem::size_of::<GptEntry>();
    let entry_size = header.entry_size as usize;
    let per_sector = (sector_size as usize) / entry_size;
    if per_sector == 0 {
        return Err(GptError::EntrySizeExceedsSector {
            entry_size: header.entry_size,
            sector_size,
        }
        .into());
    }
    let mut sector = vec![0u8; sector_size as usize];

    let mut idx = 0usize;
    let mut entries_lba = header.entries_lba;
    let num_entries = header.num_entries as usize;
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
        io.write_at_lba(entries_lba, sector_size, &sector)?;
        entries_lba += 1;
        idx += take;
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Write GPT
// ---------------------------------------------------------------------

#[cfg(feature = "alloc")]
pub fn write_gpt_with_header<IO: RimIO + ?Sized>(
    io: &mut IO,
    mut header: GptHeader,
    entries: &[GptEntry],
    sector_size: u64,
) -> PartResult {
    // Base invariants
    let entry_size = header.entry_size as usize;
    let base = core::mem::size_of::<GptEntry>();
    if entry_size < base || (entry_size % 8) != 0 {
        return Err(GptError::EntrySizeInvalid {
            base: base as u32,
            got: header.entry_size,
        }
        .into());
    }
    if (sector_size as usize) < entry_size {
        return Err(GptError::EntrySizeExceedsSector {
            entry_size: header.entry_size,
            sector_size,
        }
        .into());
    }

    let total_slots = header.num_entries as usize;
    // Refuse silent > num_entries (we prefer reporting rather than clipping)
    if entries.len() > total_slots {
        return Err(PartError::Other(
            "GPT: too many entries for header.num_entries",
        ));
    }

    // CRC entries + header
    header.compute_crc32(entries);

    // Write entries + primary header
    write_entries(io, entries, &header, sector_size)?;
    io.write_struct_lba(header.current_lba, sector_size, &header)?;

    // Backup: recalculation of positions + header CRC already done by to_backup()
    let mut backup = header.to_backup(sector_size);
    backup.entries_crc32 = header.entries_crc32;

    // Write entries + backup header
    write_entries(io, entries, &backup, sector_size)?;
    io.write_struct_lba(backup.current_lba, sector_size, &backup)?;

    io.flush()?;
    Ok(())
}

#[cfg(feature = "alloc")]
pub fn write_gpt_from_entries_with_sector<IO: RimIO + ?Sized>(
    io: &mut IO,
    entries: &[GptEntry],
    sector_size: u64,
    total_sectors: u64,
    disk_guid: [u8; 16],
) -> PartResult {
    let header = GptHeader::new(sector_size, total_sectors, disk_guid)?;
    write_gpt_with_header(io, header, entries, sector_size)
}

#[cfg(feature = "alloc")]
pub fn write_gpt_from_entries<IO: RimIO + ?Sized>(
    io: &mut IO,
    entries: &[GptEntry],
    total_sectors: u64,
    disk_guid: [u8; 16],
) -> PartResult {
    write_gpt_from_entries_with_sector(io, entries, DEFAULT_SECTOR_SIZE, total_sectors, disk_guid)
}

#[cfg(feature = "alloc")]
fn parse_entries_from_region(region: &[u8], entry_size: usize) -> PartResult<Vec<GptEntry>> {
    // --- common: parse from a raw entries buffer ---
    let base = core::mem::size_of::<GptEntry>();
    if entry_size < base || (entry_size % 8) != 0 {
        return Err(GptError::EntrySizeInvalid {
            base: base as u32,
            got: entry_size as u32,
        }
        .into());
    }
    let count = region.len() / entry_size;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let slot = &region[i * entry_size..(i + 1) * entry_size];
        let head = &slot[..base];
        let e =
            GptEntry::ref_from_bytes(head).map_err(|_| PartError::Other("GPT: Invalid entry"))?;
        if !e.is_empty() {
            out.push(*e);
        }
    }
    Ok(out)
}

// --- header API (identical) ---
pub fn read_gpt_header<IO: RimIO + ?Sized>(io: &mut IO, sector_size: u64) -> PartResult<GptHeader> {
    let hdr: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;
    hdr.validate_header()?;
    Ok(hdr)
}

// --- alloc: reads the raw region once, checks CRC, then parses ---
#[cfg(feature = "alloc")]
pub fn read_gpt_entries<IO: RimIO + ?Sized>(
    io: &mut IO,
    hdr: &GptHeader,
    sector_size: u64,
) -> PartResult<Vec<GptEntry>> {
    let entry_size = hdr.entry_size as usize;
    let num_entries = hdr.num_entries as usize;

    let buf_len = num_entries
        .checked_mul(entry_size)
        .ok_or(PartError::Other("GPT: entries byte length overflow"))?;

    let mut region = vec![0u8; buf_len];
    io.read_at_lba(hdr.entries_lba, sector_size, &mut region)?;

    let calc = compute_entries_crc32_from_iter(
        region.chunks(entry_size).map(|chunk| {
            let mut buf = [0u8; core::mem::size_of::<GptEntry>()];
            let len = buf.len();
            buf.copy_from_slice(&chunk[..len]);
            buf
        }),
        hdr,
    );

    if calc != hdr.entries_crc32 {
        return Err(GptError::CrcEntriesMismatch {
            expected: hdr.entries_crc32,
            found: calc,
        }
        .into());
    }

    // Parse
    parse_entries_from_region(&region, entry_size)
}

// --- read_gpt_at_lba refactoring: no more duplicates ---
#[cfg(feature = "alloc")]
fn read_gpt_at_lba<IO: RimIO + ?Sized>(
    io: &mut IO,
    header_lba: u64,
    sector_size: u64,
) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    let hdr: GptHeader = io.read_struct_lba(header_lba, sector_size)?;
    hdr.validate_header()?;

    let entries = read_gpt_entries(io, &hdr, sector_size)?;
    // No need to re-check CRC here (already done in read_gpt_entries),
    // but we keep logical validations:
    hdr.validate_entries(&entries, sector_size)?;

    Ok((hdr, entries))
}

#[cfg(feature = "alloc")]
pub fn read_gpt_with_sector<IO: RimIO + ?Sized>(
    io: &mut IO,
    sector_size: u64,
) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    match read_gpt_at_lba(io, GPT_PRIMARY_HEADER_LBA, sector_size) {
        Ok(ok) => Ok(ok),
        Err(_e_primary) => {
            // fallback: just to get the backup_lba
            let raw_primary: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;
            read_gpt_at_lba(io, raw_primary.backup_lba, sector_size)
        }
    }
}

#[cfg(feature = "alloc")]
pub fn read_gpt<IO: RimIO + ?Sized>(io: &mut IO) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    read_gpt_with_sector(io, crate::DEFAULT_SECTOR_SIZE)
}

/// Place des entrées à la suite, alignées 1 MiB, dans les bornes du header.
/// Retourne une `Vec<GptEntry>` ou une erreur si ça ne rentre pas.
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
    let max_slots = header.num_entries as usize;

    for (typ, uid, len_sectors, attrs, name) in reqs {
        // too many entries for the table
        if out.len() >= max_slots {
            return Err(PartError::Other("GPT: not enough entry slots"));
        }
        // zero-sized allocation
        if len_sectors == 0 {
            return Err(PartError::Other("GPT: zero-sized allocation"));
        }
        // align the beginning
        if cur % align != 0 {
            cur += align - (cur % align);
        }
        // bound after alignment
        if cur > header.last_usable_lba {
            return Err(GptError::EntryOutOfBounds {
                first_usable: header.first_usable_lba,
                last_usable: header.last_usable_lba,
                start: cur,
                end: cur,
            }
            .into());
        }

        // fin avec overflow-check
        let end = cur
            .checked_add(len_sectors - 1)
            .ok_or(GptError::LbaOverflow)?;

        // end bound
        if end > header.last_usable_lba {
            return Err(GptError::EntryOutOfBounds {
                first_usable: header.first_usable_lba,
                last_usable: header.last_usable_lba,
                start: cur,
                end,
            }
            .into());
        }

        out.push(GptEntry::new(*typ, *uid, cur, end, attrs, name));
        cur = end.saturating_add(1);
    }

    Ok(out)
}

/// Place des entrées alignées 1 MiB dans les bornes du header.
/// Variante "best-effort": arrête proprement quand ça ne rentre plus,
/// au lieu de retourner une erreur. Les entrées déjà placées restent valides.
///
/// - Respecte header.num_entries comme plafond de slots.
/// - Conserve tous les contrôles (align, overflow, bornes).
#[cfg(feature = "alloc")]
pub fn make_aligned_entries_fit<'a, I>(
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
    if cur % align != 0 {
        cur += align - (cur % align);
    }

    let mut out = vec![];
    let max_slots = header.num_entries as usize;

    for (typ, uid, len_sectors, attrs, name) in reqs {
        if out.len() >= max_slots {
            break;
        }
        if len_sectors == 0 {
            // gently ignore zero sizes (no entry)
            continue;
        }

        // realign if needed
        if cur % align != 0 {
            cur += align - (cur % align);
        }
        if cur > header.last_usable_lba {
            break; // plus d’espace
        }

        // end with overflow-check
        let Some(end) = cur.checked_add(len_sectors - 1) else {
            // overflow → stop (no more usable space anyway)
            break;
        };

        // if it exceeds bounds, stop WITHOUT error (best-effort)
        if end > header.last_usable_lba {
            break;
        }

        out.push(GptEntry::new(*typ, *uid, cur, end, attrs, name));

        // next position (with re-align later)
        cur = end.saturating_add(1);
    }

    Ok(out)
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;

    #[test]
    fn write_and_parse_gpt() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = MemRimIO::new(&mut buf);

        let part = GptEntry::new([1; 16], [2; 16], 2048, 4095, 0, "test");

        write_gpt_from_entries(&mut io, &[part], 20_000, [0xAB; 16]).unwrap();

        let (header, parts) = read_gpt(&mut io).unwrap();
        assert_eq!(header.signature, *GPT_SIGNATURE);
        assert_eq!(header.disk_guid, [0xAB; 16]);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].start_lba, 2048);
    }

    #[test]
    fn overlap_detection() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = MemRimIO::new(&mut buf);

        let p1 = GptEntry::new([1; 16], [2; 16], 2048, 6143, 0, "1");
        let p2 = GptEntry::new([3; 16], [4; 16], 4096, 8191, 0, "2");
        write_gpt_from_entries(&mut io, &[p1, p2], 20_000, [0xAB; 16]).unwrap();
        assert!(read_gpt(&mut io).is_err());
    }

    #[test]
    fn entry_size_exceeds_sector() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = MemRimIO::new(&mut buf);
        let mut hdr = GptHeader::new(512, 20_000, [0u8; 16]).unwrap();
        // force an aberrant entry size
        hdr.entry_size = 1024;
        let e = write_entries(&mut io, &[], &hdr, 512).unwrap_err();
        assert!(matches!(
            e,
            PartError::Gpt(GptError::EntrySizeExceedsSector { .. })
        ));
    }

    #[test]
    fn crc_iter_over_entries_equals_iter_over_region() {
        let mut buf = vec![0u8; 512 * 20_000];
        let mut io = MemRimIO::new(&mut buf);

        let mut hdr = GptHeader::new(512, 20_000, [0xAB; 16]).unwrap();
        let parts = vec![
            GptEntry::new([1; 16], [2; 16], 2048, 4095, 0, "A"),
            GptEntry::new([3; 16], [4; 16], 4096, 8191, 0, "B"),
        ];
        hdr.compute_crc32(&parts);
        write_gpt_from_entries_with_sector(&mut io, &parts, 512, 20_000, [0xAB; 16]).unwrap();

        let es = hdr.entry_size as usize;
        let ne = hdr.num_entries as usize;
        let mut region = vec![0u8; es * ne];
        io.read_at_lba(hdr.entries_lba, 512, &mut region).unwrap();

        let crc_entries = compute_entries_crc32_from_iter(parts.iter().map(entry_head_bytes), &hdr);
        let crc_region = compute_entries_crc32_from_iter(
            region.chunks(es).map(|chunk| {
                let mut buf = [0u8; core::mem::size_of::<GptEntry>()];
                let len = buf.len();
                buf.copy_from_slice(&chunk[..len]);
                buf
            }),
            &hdr,
        );
        assert_eq!(crc_entries, crc_region);
    }
}
