// SPDX-License-Identifier: MIT

//! GUID Partition Table (GPT) structures, CRC32 verification, and layout calculation.

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "alloc")]
use alloc::string::String;
#[cfg(feature = "alloc")]
use alloc::vec;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

#[cfg(feature = "alloc")]
use crate::DEFAULT_SECTOR_SIZE;
use crate::guids::GptPartitionKind;
#[cfg(feature = "alloc")]
use crate::io_ext::RimWriteLbaExt;
use crate::{errors::*, io_ext::RimReadLbaExt};
use rimio::prelude::*;
use zerocopy::byteorder::little_endian::{U16, U32, U64};
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
    if v.is_multiple_of(a) {
        v
    } else {
        v + (a - (v % a))
    }
}
#[inline]
pub fn align_down(v: u64, a: u64) -> u64 {
    v - (v % a)
}

#[inline]
pub fn align_lba_1m(sector_size: u64) -> u64 {
    ((1 << 20) / sector_size).max(1)
}

pub fn encode_gpt_name(name: &str) -> [U16; 36] {
    let mut buf = [U16::ZERO; 36];
    for (i, c) in name.encode_utf16().take(36).enumerate() {
        buf[i] = c.into();
    }
    buf
}

#[cfg(feature = "alloc")]
pub fn decode_gpt_name(name: &[U16; 36]) -> String {
    core::char::decode_utf16(name.iter().map(|c| c.get()).take_while(|&c| c != 0))
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

#[cfg(not(feature = "alloc"))]
pub fn decode_gpt_name<'a>(name: &[U16; 36], buf: &'a mut [u8]) -> Result<&'a str, PartError> {
    let mut written = 0;
    for ch in core::char::decode_utf16(name.iter().map(|c| c.get()).take_while(|&c| c != 0)) {
        let ch = ch.unwrap_or(char::REPLACEMENT_CHARACTER);
        let end = written + ch.len_utf8();
        let dest = buf
            .get_mut(written..end)
            .ok_or(PartError::Other("GPT: name buffer too small"))?;
        ch.encode_utf8(dest);
        written = end;
    }
    core::str::from_utf8(&buf[..written]).map_err(|_| PartError::Other("UTF-8 error"))
}

#[inline]
fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

#[inline]
fn compute_header_crc32(mut header: GptHeader) -> u32 {
    header.header_crc32 = (0).into();
    let bytes = header.as_bytes();
    crc32(&bytes[..header.header_size.get() as usize])
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C)]
pub struct GptEntry {
    pub type_guid: [u8; 16],
    pub unique_guid: [u8; 16],
    pub start_lba: U64,
    pub end_lba: U64,
    pub attributes: U64,
    pub name: [U16; 36],
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
            start_lba: start_lba.into(),
            end_lba: end_lba.into(),
            attributes: attributes.into(),
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
            && self.start_lba.get() == 0
            && self.end_lba.get() == 0
            && self.attributes.get() == 0
            && self.name.iter().all(|c| c.get() == 0)
    }

    pub fn validate(&self) -> PartResult<()> {
        if self.is_empty() {
            return Ok(());
        }
        if self.start_lba.get() > self.end_lba.get() {
            return Err(PartError::Other("GPT: entry start > end"));
        }
        Ok(())
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C)]
pub struct GptHeader {
    pub signature: [u8; 8],
    pub revision: U32,
    pub header_size: U32,
    pub header_crc32: U32,
    pub reserved: U32,
    pub current_lba: U64,
    pub backup_lba: U64,
    pub first_usable_lba: U64,
    pub last_usable_lba: U64,
    pub disk_guid: [u8; 16],
    pub entries_lba: U64,
    pub num_entries: U32,
    pub entry_size: U32,
    pub entries_crc32: U32,
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
            revision: (GPT_REVISION).into(),
            header_size: (GPT_DEFAULT_HEADER_SIZE).into(),
            header_crc32: (0).into(),
            reserved: (0).into(),
            current_lba: (GPT_PRIMARY_HEADER_LBA).into(),
            backup_lba: (total_sectors - 1).into(),
            first_usable_lba: first_usable_lba.into(),
            last_usable_lba: last_usable_lba.into(),
            disk_guid,
            entries_lba: entries_lba.into(),
            num_entries: (GPT_DEFAULT_NUM_ENTRIES).into(),
            entry_size: (GPT_DEFAULT_ENTRY_SIZE).into(),
            entries_crc32: (0).into(),
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
        if entry_size < base_es || !entry_size.is_multiple_of(8) {
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
            revision: (GPT_REVISION).into(),
            header_size: (GPT_DEFAULT_HEADER_SIZE).into(),
            header_crc32: (0).into(),
            reserved: (0).into(),
            current_lba: (GPT_PRIMARY_HEADER_LBA).into(),
            backup_lba: (total_sectors - 1).into(),
            first_usable_lba: first_usable_lba.into(),
            last_usable_lba: last_usable_lba.into(),
            disk_guid,
            entries_lba: entries_lba.into(),
            num_entries: num_entries.into(),
            entry_size: entry_size.into(),
            entries_crc32: (0).into(),
            reserved2: [0u8; 420],
        })
    }

    pub fn total_sectors(&self) -> u64 {
        self.backup_lba.get() + 1
    }

    pub fn to_backup(mut self, sector_size: u64) -> Self {
        self.current_lba = (self.backup_lba.get()).into();
        self.backup_lba = (GPT_PRIMARY_HEADER_LBA).into();

        let entries_sectors =
            (self.num_entries.get() as u64 * self.entry_size.get() as u64).div_ceil(sector_size);

        self.entries_lba = (self.current_lba.get() - entries_sectors).into();
        self.header_crc32 = (compute_header_crc32(self)).into();
        self
    }

    pub fn compute_crc32(&mut self, entries: &[GptEntry]) {
        self.entries_crc32 =
            (compute_entries_crc32_from_iter(entries.iter().map(entry_head_bytes), self)).into();
        self.header_crc32 = (compute_header_crc32(*self)).into();
    }

    pub fn validate_header(&self) -> PartResult {
        if &self.signature != GPT_SIGNATURE {
            return Err(GptError::InvalidSignature {
                expected: *GPT_SIGNATURE,
                found: self.signature,
            }
            .into());
        }
        if self.revision.get() != GPT_REVISION {
            return Err(GptError::InvalidRevision {
                expected: GPT_REVISION,
                found: self.revision.get(),
            }
            .into());
        }
        if self.header_size.get() < GPT_DEFAULT_HEADER_SIZE {
            return Err(GptError::HeaderSizeTooSmall {
                min: GPT_DEFAULT_HEADER_SIZE,
                got: self.header_size.get(),
            }
            .into());
        }
        let max_hdr = core::mem::size_of::<GptHeader>();
        if (self.header_size.get() as usize) > max_hdr {
            return Err(GptError::HeaderSizeTooLarge {
                max: max_hdr,
                got: self.header_size.get(),
            }
            .into());
        }
        if self.entry_size.get() > 512 {
            return Err(GptError::EntrySizeTooLarge {
                max: 512,
                got: self.entry_size.get(),
            }
            .into());
        }
        if self.num_entries.get() == 0 || self.num_entries.get() > 16_384 {
            return Err(GptError::NumEntriesOutOfRange {
                min: 1,
                max: 16_384,
                got: self.num_entries.get(),
            }
            .into());
        }
        let base_es = core::mem::size_of::<GptEntry>() as u32;
        if self.entry_size.get() < base_es || !self.entry_size.get().is_multiple_of(8) {
            return Err(GptError::EntrySizeInvalid {
                base: base_es,
                got: self.entry_size.get(),
            }
            .into());
        }
        let calc = compute_header_crc32(*self);
        if calc != self.header_crc32.get() {
            return Err(GptError::CrcHeaderMismatch {
                expected: self.header_crc32.get(),
                found: calc,
            }
            .into());
        }
        Ok(())
    }

    pub fn validate_entry(&self, entry: &GptEntry, sector_size: u64) -> PartResult {
        entry.validate()?;

        let align = align_lba_1m(sector_size);
        let first_usable = self.first_usable_lba.get();
        let last_usable = self.last_usable_lba.get();

        if entry.start_lba.get() < first_usable || entry.end_lba.get() > last_usable {
            return Err(GptError::EntryOutOfBounds {
                first_usable,
                last_usable,
                start: entry.start_lba.get(),
                end: entry.end_lba.get(),
            }
            .into());
        }

        if !entry.start_lba.get().is_multiple_of(align) {
            return Err(GptError::EntryUnaligned {
                lba: entry.start_lba.get(),
                align,
            }
            .into());
        }
        Ok(())
    }

    pub fn validate_entries(&self, entries: &[GptEntry], sector_size: u64) -> PartResult {
        let calc = compute_entries_crc32_from_iter(entries.iter().map(entry_head_bytes), self);
        if calc != self.entries_crc32.get() {
            return Err(GptError::CrcEntriesMismatch {
                expected: self.entries_crc32.get(),
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
        .map(|e| (e.start_lba.get(), e.end_lba.get()))
        .collect();

    if segs.len() <= 1 {
        return Ok(());
    }

    segs.sort_unstable_by_key(|a| a.0);

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
            if overlaps_inclusive(
                a.start_lba.get(),
                a.end_lba.get(),
                b.start_lba.get(),
                b.end_lba.get(),
            ) {
                return Err(GptError::Overlap {
                    a_start: a.start_lba.get(),
                    a_end: a.end_lba.get(),
                    b_start: b.start_lba.get(),
                    b_end: b.end_lba.get(),
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
    let entry_size = header.entry_size.get() as usize;
    debug_assert!(entry_size >= base && entry_size.is_multiple_of(8));

    let mut hasher = crc32fast::Hasher::new();

    let mut slot = [0u8; 512];
    let mut produced = 0usize;
    let num_entries = header.num_entries.get() as usize;

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
    let entry_size = header.entry_size.get() as usize;
    let per_sector = (sector_size as usize) / entry_size;
    if per_sector == 0 {
        return Err(GptError::EntrySizeExceedsSector {
            entry_size: header.entry_size.get(),
            sector_size,
        }
        .into());
    }
    let mut sector = vec![0u8; sector_size as usize];

    let mut idx = 0usize;
    let mut entries_lba = header.entries_lba.get();
    let num_entries = header.num_entries.get() as usize;
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

#[cfg(feature = "alloc")]
pub fn write_gpt_with_header<IO: RimIO + ?Sized>(
    io: &mut IO,
    mut header: GptHeader,
    entries: &[GptEntry],
    sector_size: u64,
) -> PartResult {
    // Base invariants
    let entry_size = header.entry_size.get() as usize;
    let base = core::mem::size_of::<GptEntry>();
    if entry_size < base || !entry_size.is_multiple_of(8) {
        return Err(GptError::EntrySizeInvalid {
            base: base as u32,
            got: header.entry_size.get(),
        }
        .into());
    }
    if (sector_size as usize) < entry_size {
        return Err(GptError::EntrySizeExceedsSector {
            entry_size: header.entry_size.get(),
            sector_size,
        }
        .into());
    }

    let total_slots = header.num_entries.get() as usize;
    // Refuse silent > num_entries (we prefer reporting rather than clipping)
    if entries.len() > total_slots {
        return Err(PartError::Other(
            "GPT: too many entries for header.num_entries.get()",
        ));
    }

    // CRC entries + header
    header.compute_crc32(entries);

    write_entries(io, entries, &header, sector_size)?;
    io.write_struct_lba(header.current_lba.get(), sector_size, &header)?;

    // Backup: recalculation of positions + header CRC already done by to_backup()
    let mut backup = header.to_backup(sector_size);
    backup.entries_crc32 = (header.entries_crc32.get()).into();

    write_entries(io, entries, &backup, sector_size)?;
    io.write_struct_lba(backup.current_lba.get(), sector_size, &backup)?;

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
    let base = core::mem::size_of::<GptEntry>();
    if entry_size < base || !entry_size.is_multiple_of(8) {
        return Err(GptError::EntrySizeInvalid {
            base: base as u32,
            got: entry_size as u32,
        }
        .into());
    }
    if !region.len().is_multiple_of(entry_size) {
        return Err(PartError::Other("GPT: truncated entry table"));
    }
    if entry_size == base {
        let entries = <[GptEntry]>::ref_from_bytes(region)
            .map_err(|_| PartError::Other("GPT: Invalid entry table"))?;
        return Ok(entries.iter().filter(|e| !e.is_empty()).copied().collect());
    }
    let mut out = Vec::with_capacity(region.len() / entry_size);
    for slot in region.chunks_exact(entry_size) {
        let (entry, _) =
            GptEntry::ref_from_prefix(slot).map_err(|_| PartError::Other("GPT: Invalid entry"))?;
        if !entry.is_empty() {
            out.push(*entry);
        }
    }
    Ok(out)
}

pub fn read_gpt_header<IO: RimRead + ?Sized>(
    io: &mut IO,
    sector_size: u64,
) -> PartResult<GptHeader> {
    let hdr: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;
    hdr.validate_header()?;
    Ok(hdr)
}

#[cfg(feature = "alloc")]
pub fn read_gpt_entries<IO: RimRead + ?Sized>(
    io: &mut IO,
    hdr: &GptHeader,
    sector_size: u64,
) -> PartResult<Vec<GptEntry>> {
    let entry_size = hdr.entry_size.get() as usize;
    let num_entries = hdr.num_entries.get() as usize;

    let buf_len = num_entries
        .checked_mul(entry_size)
        .ok_or(PartError::Other("GPT: entries byte length overflow"))?;

    let mut region = vec![0u8; buf_len];
    io.read_at_lba(hdr.entries_lba.get(), sector_size, &mut region)?;

    // The checksum covers the complete slots, including unknown extension bytes.
    let calc = crc32(&region);

    if calc != hdr.entries_crc32.get() {
        return Err(GptError::CrcEntriesMismatch {
            expected: hdr.entries_crc32.get(),
            found: calc,
        }
        .into());
    }

    parse_entries_from_region(&region, entry_size)
}

#[cfg(feature = "alloc")]
fn read_gpt_at_lba<IO: RimRead + ?Sized>(
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
pub fn read_gpt_with_sector<IO: RimRead + ?Sized>(
    io: &mut IO,
    sector_size: u64,
) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    match read_gpt_at_lba(io, GPT_PRIMARY_HEADER_LBA, sector_size) {
        Ok(ok) => Ok(ok),
        Err(_e_primary) => {
            // fallback: just to get the backup_lba
            let raw_primary: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;
            read_gpt_at_lba(io, raw_primary.backup_lba.get(), sector_size)
        }
    }
}

#[cfg(feature = "alloc")]
pub fn read_gpt<IO: RimRead + ?Sized>(io: &mut IO) -> PartResult<(GptHeader, Vec<GptEntry>)> {
    read_gpt_with_sector(io, crate::DEFAULT_SECTOR_SIZE)
}

/// Places entries sequentially, aligned to 1 MiB, within the header bounds.
/// Returns a `Vec<GptEntry>` or an error if it doesn't fit.
/// Designed for tests and simple cases (no imposed intervals).
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
    let mut cur = header.first_usable_lba.get();
    let mut out = vec![];
    let max_slots = header.num_entries.get() as usize;

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
        if !cur.is_multiple_of(align) {
            cur += align - (cur % align);
        }
        // bound after alignment
        if cur > header.last_usable_lba.get() {
            return Err(GptError::EntryOutOfBounds {
                first_usable: header.first_usable_lba.get(),
                last_usable: header.last_usable_lba.get(),
                start: cur,
                end: cur,
            }
            .into());
        }

        // end with overflow-check
        let end = cur
            .checked_add(len_sectors - 1)
            .ok_or(GptError::LbaOverflow)?;

        // end bound
        if end > header.last_usable_lba.get() {
            return Err(GptError::EntryOutOfBounds {
                first_usable: header.first_usable_lba.get(),
                last_usable: header.last_usable_lba.get(),
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

/// Places 1 MiB-aligned entries within header bounds using a best-effort strategy,
/// stopping gracefully when capacity is reached instead of returning an error.
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
    let mut cur = header.first_usable_lba.get();
    if !cur.is_multiple_of(align) {
        cur += align - (cur % align);
    }

    let mut out = vec![];
    let max_slots = header.num_entries.get() as usize;

    for (typ, uid, len_sectors, attrs, name) in reqs {
        if out.len() >= max_slots {
            break;
        }
        if len_sectors == 0 {
            // gently ignore zero sizes (no entry)
            continue;
        }

        // realign if needed
        if !cur.is_multiple_of(align) {
            cur += align - (cur % align);
        }
        if cur > header.last_usable_lba.get() {
            break;
        }

        let Some(end) = cur.checked_add(len_sectors - 1) else {
            // overflow → stop (no more usable space anyway)
            break;
        };

        // if it exceeds bounds, stop WITHOUT error (best-effort)
        if end > header.last_usable_lba.get() {
            break;
        }

        out.push(GptEntry::new(*typ, *uid, cur, end, attrs, name));
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
        assert_eq!(parts[0].start_lba.get(), 2048);
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
        hdr.entry_size = (1024).into();
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

        let es = hdr.entry_size.get() as usize;
        let ne = hdr.num_entries.get() as usize;
        let mut region = vec![0u8; es * ne];
        io.read_at_lba(hdr.entries_lba.get(), 512, &mut region)
            .unwrap();

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

const _: () = {
    assert!(core::mem::size_of::<GptEntry>() == 128);
    assert!(core::mem::align_of::<GptEntry>() == 1);
    assert!(core::mem::offset_of!(GptEntry, unique_guid) == 16);
    assert!(core::mem::offset_of!(GptEntry, start_lba) == 32);
    assert!(core::mem::offset_of!(GptEntry, end_lba) == 40);
    assert!(core::mem::offset_of!(GptEntry, attributes) == 48);
    assert!(core::mem::offset_of!(GptEntry, name) == 56);
    assert!(core::mem::size_of::<GptHeader>() == 512);
    assert!(core::mem::align_of::<GptHeader>() == 1);
    assert!(core::mem::offset_of!(GptHeader, revision) == 8);
    assert!(core::mem::offset_of!(GptHeader, header_size) == 12);
    assert!(core::mem::offset_of!(GptHeader, header_crc32) == 16);
    assert!(core::mem::offset_of!(GptHeader, reserved) == 20);
    assert!(core::mem::offset_of!(GptHeader, current_lba) == 24);
    assert!(core::mem::offset_of!(GptHeader, backup_lba) == 32);
    assert!(core::mem::offset_of!(GptHeader, first_usable_lba) == 40);
    assert!(core::mem::offset_of!(GptHeader, last_usable_lba) == 48);
    assert!(core::mem::offset_of!(GptHeader, disk_guid) == 56);
    assert!(core::mem::offset_of!(GptHeader, entries_lba) == 72);
    assert!(core::mem::offset_of!(GptHeader, num_entries) == 80);
    assert!(core::mem::offset_of!(GptHeader, entry_size) == 84);
    assert!(core::mem::offset_of!(GptHeader, entries_crc32) == 88);
    assert!(core::mem::offset_of!(GptHeader, reserved2) == 92);
};

#[cfg(test)]
mod layout_tests {
    use super::*;
    #[test]
    fn gpt_entry_is_little_endian_and_borrowable_at_any_offset() {
        let entry = GptEntry::new(
            [1; 16],
            [2; 16],
            0x0102030405060708,
            0x1112131415161718,
            0x2122232425262728,
            "A\u{1f600}",
        );
        assert_eq!(&entry.as_bytes()[32..40], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&entry.as_bytes()[56..62], &[65, 0, 0x3d, 0xd8, 0, 0xde]);
        let mut unaligned = [0u8; 129];
        unaligned[1..].copy_from_slice(entry.as_bytes());
        let parsed = GptEntry::ref_from_bytes(&unaligned[1..]).unwrap();
        assert_eq!(parsed.as_bytes(), entry.as_bytes());
        assert!(GptEntry::ref_from_bytes(&unaligned[1..128]).is_err());
        #[cfg(feature = "alloc")]
        assert_eq!(decode_gpt_name(&parsed.name), "A\u{1f600}");
        #[cfg(not(feature = "alloc"))]
        {
            let mut out = [0; 5];
            assert_eq!(
                decode_gpt_name(&parsed.name, &mut out).unwrap(),
                "A\u{1f600}"
            );
            assert!(decode_gpt_name(&parsed.name, &mut [0; 4]).is_err());
        }
    }

    #[test]
    fn streaming_reader_uses_fixed_buffers_without_alloc() {
        use crate::io_ext::RimWriteLbaExt;
        let entry = GptEntry::new([1; 16], [2; 16], 2048, 4095, 0, "A");
        let mut table = [0u8; 512];
        table[..128].copy_from_slice(entry.as_bytes());
        let mut header = GptHeader::new_with_table(512, 20_000, [3; 16], 4, 128).unwrap();
        header.entries_crc32 = crc32(&table).into();
        header.header_crc32 = compute_header_crc32(header).into();
        let mut disk = [0u8; 2048];
        let mut io = rimio::MemRimIO::new(&mut disk);
        io.write_struct_lba(1, 512, &header).unwrap();
        io.write_at_lba(2, 512, &table).unwrap();
        let mut reader = crate::gpt_stream::GptStreamReader::<_, 512>::new(&mut io, 512).unwrap();
        reader.validate_crc().unwrap();
        let (_, parsed) = reader.find_first(|e| !e.is_empty()).unwrap().unwrap();
        assert_eq!(parsed.as_bytes(), entry.as_bytes());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn gpt_crc_covers_extended_slot_bytes_in_both_readers() {
        let mut disk = alloc::vec![0u8; 4096];
        let entry = GptEntry::new([1; 16], [2; 16], 2048, 4095, 0, "A");
        let mut region = [0u8; 144 * 4];
        region[..128].copy_from_slice(entry.as_bytes());
        region[128..144].fill(0x5a);
        let mut header = GptHeader::new_with_table(512, 20_000, [3; 16], 4, 144).unwrap();
        header.entries_crc32 = crc32(&region).into();
        header.header_crc32 = compute_header_crc32(header).into();
        let mut io = rimio::MemRimIO::new(&mut disk);
        io.write_struct_lba(1, 512, &header).unwrap();
        io.write_at_lba(header.entries_lba.get(), 512, &region)
            .unwrap();
        assert_eq!(read_gpt_entries(&mut io, &header, 512).unwrap().len(), 1);
        crate::gpt_stream::GptStreamReader::<_, 512>::new(&mut io, 512)
            .unwrap()
            .validate_crc()
            .unwrap();
        region[130] ^= 1;
        io.write_at_lba(header.entries_lba.get(), 512, &region)
            .unwrap();
        assert!(read_gpt_entries(&mut io, &header, 512).is_err());
        assert!(
            crate::gpt_stream::GptStreamReader::<_, 512>::new(&mut io, 512)
                .unwrap()
                .validate_crc()
                .is_err()
        );
        assert!(parse_entries_from_region(&region[..143], 144).is_err());
    }
}
