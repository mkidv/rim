// SPDX-License-Identifier: MIT
#![allow(dead_code)]

use crate::errors::*;
use crate::gpt::{GPT_PRIMARY_HEADER_LBA, GptEntry, GptHeader, overlaps_inclusive};
use crate::io_ext::RimIOLbaExt;
use crc32fast::Hasher;
use rimio::prelude::*;
use zerocopy::{FromBytes, IntoBytes};

/// No-allocation GPT cursor: reads entries one by one with a stack buffer.
#[derive(Debug)]
pub struct GptStreamReader<'io, IO: RimIO + ?Sized, const N: usize> {
    io: &'io mut IO,
    header: GptHeader,
    sector_size: u64,
    entry_size: usize,
    entry_buf: [u8; N],
    sector_buf: [u8; N],
    cached_lba: Option<u64>,
}

impl<'io, IO: RimIO + ?Sized, const N: usize> GptStreamReader<'io, IO, N> {
    /// Construction: reads the primary header, verifies sizes.
    pub fn new(io: &'io mut IO, sector_size: u64) -> PartResult<Self> {
        if sector_size as usize > N {
            return Err(PartError::Other("GPT: sector_size exceeds stack buffer"));
        }

        let hdr: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;
        hdr.validate_header()?;

        let entry_size = hdr.entry_size as usize;
        if entry_size > N {
            return Err(PartError::Other("GPT: entry_size exceeds stack buffer"));
        }

        Ok(Self {
            io,
            header: hdr,
            sector_size,
            entry_size,
            entry_buf: [0u8; N],
            sector_buf: [0u8; N],
            cached_lba: None,
        })
    }

    #[inline]
    pub fn header(&self) -> &GptHeader {
        &self.header
    }

    #[inline]
    pub fn slots(&self) -> usize {
        self.header.num_entries as usize
    }

    pub fn iter<'c>(&'c mut self) -> GptIter<'c, 'io, IO, N> {
        GptIter::new(self, 0, self.slots())
    }

    /// Reads an arbitrary entry (copies to internal buffer).
    fn read_at(&mut self, index: usize) -> PartResult<GptEntry> {
        let off = index as u64 * self.entry_size as u64;
        let base_lba = self.header.entries_lba + (off / self.sector_size);
        let in_sector = (off % self.sector_size) as usize;
        let entry_size = self.entry_size;
        let ss = self.sector_size as usize;

        if entry_size > N || ss > N {
            return Err(PartError::Other("GPT: entry/sector exceeds stack buffer"));
        }

        // Case 1: entry fits in a single sector
        if in_sector + entry_size <= ss {
            // cache
            if self.cached_lba != Some(base_lba) {
                self.io
                    .read_at_lba(base_lba, self.sector_size, &mut self.sector_buf[..ss])?;
                self.cached_lba = Some(base_lba);
            }
            self.entry_buf[..entry_size]
                .copy_from_slice(&self.sector_buf[in_sector..in_sector + entry_size]);
        } else {
            // Case 2: overlap on 2 sectors
            // Read first sector (with cache)
            if self.cached_lba != Some(base_lba) {
                self.io
                    .read_at_lba(base_lba, self.sector_size, &mut self.sector_buf[..ss])?;
                self.cached_lba = Some(base_lba);
            }
            let first = ss - in_sector;
            self.entry_buf[..first].copy_from_slice(&self.sector_buf[in_sector..ss]);

            // Read second sector (not the same LBA, cache is replaced)
            let next_lba = base_lba + 1;
            self.io
                .read_at_lba(next_lba, self.sector_size, &mut self.sector_buf[..ss])?;
            self.cached_lba = Some(next_lba);
            self.entry_buf[first..entry_size]
                .copy_from_slice(&self.sector_buf[..(entry_size - first)]);
        }

        let base = core::mem::size_of::<GptEntry>();
        let e = GptEntry::ref_from_bytes(&self.entry_buf[..base])
            .map_err(|_| PartError::Other("GPT: invalid entry"))?;
        Ok(*e)
    }

    /// Iterate via callback (ignores empty slots).
    pub fn for_each_entry<F>(&mut self, mut f: F) -> PartResult<()>
    where
        F: FnMut(usize, &GptEntry) -> PartResult<()>,
    {
        for (idx, res) in self.iter().enumerate() {
            let e = res?;
            f(idx, &e)?;
        }
        Ok(())
    }

    /// Searches for the first entry matching a predicate.
    pub fn find_first<F>(&mut self, mut pred: F) -> PartResult<Option<(usize, GptEntry)>>
    where
        F: FnMut(&GptEntry) -> bool,
    {
        let len = self.slots();
        for i in 0..len {
            let e = self.read_at(i)?;
            if !e.is_empty() && pred(&e) {
                return Ok(Some((i, e)));
            }
        }
        Ok(None)
    }

    /// Validation bornes/alignment en streaming.
    pub fn validate_bounds(&mut self) -> PartResult<()> {
        for i in 0..self.slots() {
            let entry = self.read_at(i)?;
            if entry.is_empty() {
                continue;
            }
            self.header.validate_entry(&entry, self.sector_size)?;
        }
        Ok(())
    }

    /// O(n²) overlap detection in streaming (no Vec).
    pub fn validate_overlaps(&mut self) -> PartResult<()> {
        let n = self.slots();
        for i in 0..n {
            let ei = self.read_at(i)?;
            if ei.is_empty() {
                continue;
            }
            for j in (i + 1)..n {
                let ej = self.read_at(j)?;
                if ej.is_empty() {
                    continue;
                }
                if overlaps_inclusive(ei.start_lba, ei.end_lba, ej.start_lba, ej.end_lba) {
                    return Err(GptError::Overlap {
                        a_start: ei.start_lba,
                        a_end: ei.end_lba,
                        b_start: ej.start_lba,
                        b_end: ej.end_lba,
                    }
                    .into());
                }
            }
        }
        Ok(())
    }

    pub fn validate_crc(&mut self) -> PartResult<()> {
        let ss = self.sector_size as usize;
        if ss > N {
            return Err(PartError::Other("GPT: sector_size exceeds stack buffer"));
        }

        let total_bytes = (self.header.num_entries as usize)
            .checked_mul(self.entry_size)
            .ok_or(PartError::Other("GPT: entries byte length overflow"))?;

        let mut remaining = total_bytes;
        let mut lba = self.header.entries_lba;
        let mut hasher = Hasher::new();

        while remaining > 0 {
            // lire 1 secteur
            self.io
                .read_at_lba(lba, self.sector_size, &mut self.sector_buf[..ss])?;
            let take = core::cmp::min(remaining, ss);
            hasher.update(&self.sector_buf[..take]);
            remaining -= take;
            lba += 1;
        }

        let calc = hasher.finalize();
        if calc != self.header.entries_crc32 {
            return Err(GptError::CrcEntriesMismatch {
                expected: self.header.entries_crc32,
                found: calc,
            }
            .into());
        }
        Ok(())
    }

    pub fn collect_into(&mut self, out: &mut [GptEntry]) -> PartResult<usize> {
        let mut written = 0usize;
        let len = self.slots();

        for i in 0..len {
            let e = self.read_at(i)?;
            if e.is_empty() {
                continue;
            }
            if written == out.len() {
                return Err(PartError::Other("GPT: output slice too small"));
            }
            out[written] = e;
            written += 1;
        }
        Ok(written)
    }
}

// ── Iterator pour GptCursor ───────────────────────────────────────────────────

// Iterator with two distinct lifetimes
pub struct GptIter<'c, 'io, IO: RimIO + ?Sized, const N: usize> {
    reader: &'c mut GptStreamReader<'io, IO, N>,
    pos: usize,
    len: usize,
}

impl<'c, 'io, IO: RimIO + ?Sized, const N: usize> GptIter<'c, 'io, IO, N> {
    pub fn new(reader: &'c mut GptStreamReader<'io, IO, N>, pos: usize, len: usize) -> Self {
        Self { reader, pos, len }
    }
}

// Impl de Iterator : utilise bien les deux lifetimes
impl<'c, 'io, IO: RimIO + ?Sized, const N: usize> Iterator for GptIter<'c, 'io, IO, N> {
    type Item = PartResult<GptEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.len {
            let i = self.pos;
            self.pos += 1;
            match self.reader.read_at(i) {
                Ok(e) if !e.is_empty() => return Some(Ok(e)),
                Ok(_) => continue, // skip empty
                Err(e) => return Some(Err(e)),
            }
        }
        None
    }
}

#[derive(Debug)]
pub struct GptStreamWriter<'io, IO: RimIO + ?Sized, const N: usize> {
    io: &'io mut IO,
    sector_size: u64,
    header: GptHeader,
    sector: [u8; N],   // reused
    slot: [u8; N],     // pour padded entry (<= entry_size)
    es: usize,         // entry_size
    per_sector: usize, // entries par secteur
    idx: usize,        // index courant (0..num_entries)
    crc: crc32fast::Hasher,
}

impl<'io, IO: RimIO + ?Sized, const N: usize> GptStreamWriter<'io, IO, N> {
    pub fn new(
        io: &'io mut IO,
        sector_size: u64,
        total_sectors: u64,
        disk_guid: [u8; 16],
    ) -> PartResult<Self> {
        let header = GptHeader::new(sector_size, total_sectors, disk_guid)?;
        Self::from_header(io, sector_size, header)
    }

    pub fn from_header(io: &'io mut IO, sector_size: u64, header: GptHeader) -> PartResult<Self> {
        let es = header.entry_size as usize;
        if (es % 8) != 0 {
            return Err(GptError::EntrySizeInvalid {
                base: core::mem::size_of::<GptEntry>() as u32,
                got: header.entry_size,
            }
            .into());
        }
        if sector_size as usize > N {
            return Err(PartError::Other("GPT: sector_size exceeds stack buffer"));
        }
        if es > N {
            return Err(PartError::Other("GPT: entry_size exceeds stack buffer"));
        }
        let per_sector = (sector_size as usize) / es;
        if per_sector == 0 {
            return Err(GptError::EntrySizeExceedsSector {
                entry_size: header.entry_size,
                sector_size,
            }
            .into());
        }
        Ok(Self {
            io,
            sector_size,
            header,
            sector: [0; N],
            slot: [0; N],
            es,
            per_sector,
            idx: 0,
            crc: crc32fast::Hasher::new(),
        })
    }

    pub fn write_entries<I>(&mut self, provided: usize, mut it: I) -> PartResult<()>
    where
        I: Iterator<Item = GptEntry>,
    {
        let total = self.header.num_entries as usize;
        let mut lba = self.header.entries_lba;
        let ss = self.sector_size as usize;

        // fills the entire entry table (non-empty + zeroed slots)
        let mut left = total;
        while left > 0 {
            let take = left.min(self.per_sector);
            // zero out the sector
            for b in &mut self.sector[..ss] {
                *b = 0;
            }

            for s in 0..take {
                // construire le slot (head = GptEntry, tail = 0)
                for b in &mut self.slot[..self.es] {
                    *b = 0;
                }
                if self.idx < provided {
                    let e = it
                        .next()
                        .ok_or(PartError::Other("iterator shorter than `provided`"))?;
                    let head = e.as_bytes();
                    let base = core::mem::size_of::<GptEntry>();
                    self.slot[..base].copy_from_slice(head);
                }
                self.crc.update(&self.slot[..self.es]); // CRC entries

                // place the slot in the sector at the correct location
                let dst = &mut self.sector[s * self.es..(s + 1) * self.es];
                dst.copy_from_slice(&self.slot[..self.es]);

                self.idx += 1;
            }

            self.io
                .write_at_lba(lba, self.sector_size, &self.sector[..ss])?;
            lba += 1;
            left -= take;
        }

        Ok(())
    }

    pub fn finalize(mut self) -> PartResult<()> {
        // Entries CRC
        self.header.entries_crc32 = self.crc.finalize();

        // Primary header (header_crc32 calculated on header_size, field null)
        self.header.header_crc32 = 0;
        let hcrc = {
            let bytes = self.header.as_bytes();
            crc32fast::hash(&bytes[..self.header.header_size as usize])
        };
        self.header.header_crc32 = hcrc;

        // Write primary header
        self.io
            .write_struct_lba(self.header.current_lba, self.sector_size, &self.header)?;

        // Backup
        let mut backup = self.header.to_backup(self.sector_size);
        // to_backup has already recalculated header_crc32; entries_crc32 stays identical
        backup.entries_crc32 = self.header.entries_crc32;
        backup.header_crc32 = 0;
        let bcrc = {
            let bytes = backup.as_bytes();
            crc32fast::hash(&bytes[..backup.header_size as usize])
        };
        backup.header_crc32 = bcrc;

        // recopier la zone entries vers la zone backup (streaming, secteur par secteur, sans heap)
        let ss = self.sector_size as usize;
        let mut remaining = (self.header.num_entries as usize) * self.es;
        let mut src_lba = self.header.entries_lba;
        let mut dst_lba = backup.entries_lba;

        while remaining > 0 {
            // lire un secteur source
            for b in &mut self.sector[..ss] {
                *b = 0;
            }
            self.io
                .read_at_lba(src_lba, self.sector_size, &mut self.sector[..ss])?;
            // write destination sector
            self.io
                .write_at_lba(dst_lba, self.sector_size, &self.sector[..ss])?;
            src_lba += 1;
            dst_lba += 1;
            remaining = remaining.saturating_sub(ss);
        }

        // Write backup header
        self.io
            .write_struct_lba(backup.current_lba, self.sector_size, &backup)?;

        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{gpt, guids, mbr};

    use super::*;
    use rimio::prelude::MemRimIO;

    #[test]
    fn gpt_cursor_iter_and_find_first_esp() {
        let sector = 512u64;
        let total_sectors = 20_000u64;
        let mut buf = vec![0u8; (sector * total_sectors) as usize];
        let mut io = MemRimIO::new(&mut buf);

        // Protective MBR + GPT with 2 aligned partitions
        mbr::write_mbr_protective(&mut io, total_sectors).unwrap();
        let p1 = gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_ESP, [1; 16], 2048, 4095, 0, "ESP");
        let p2 = gpt::GptEntry::new(
            guids::GPT_PARTITION_TYPE_LINUX,
            [2; 16],
            4096,
            10_000,
            0,
            "rootfs",
        );
        gpt::write_gpt_from_entries(&mut io, &[p1, p2], total_sectors, [0xAB; 16]).unwrap();

        // Cursor (stack 512)
        let mut reader = super::GptStreamReader::<_, 512>::new(&mut io, sector).unwrap();

        {
            // iter() ignores empty slots → we should see 2 entries
            let parts: Vec<_> = reader.iter().collect::<Result<Vec<_>, _>>().unwrap();
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0].start_lba, 2048);
            assert_eq!(parts[1].start_lba, 4096);
        }

        // find_first ESP
        let (idx, esp) = reader
            .find_first(|e| e.kind() == guids::GptPartitionKind::Esp)
            .unwrap()
            .expect("ESP not found");
        assert_eq!(idx, 0);
        assert_eq!(esp.end_lba, 4095);

        // validations
        reader.validate_bounds().unwrap();
        reader.validate_overlaps().unwrap();
    }

    #[test]
    fn gpt_cursor_detects_overlap() {
        let sector = 512u64;
        let total_sectors = 20_000u64;
        let mut buf = vec![0u8; (sector * total_sectors) as usize];
        let mut io = MemRimIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, total_sectors).unwrap();

        // 2 partitions qui se chevauchent
        let p1 = gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_DATA, [3; 16], 2048, 6000, 0, "A");
        let p2 = gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_DATA, [4; 16], 4096, 7000, 0, "B");
        gpt::write_gpt_from_entries(&mut io, &[p1, p2], total_sectors, [0xCD; 16]).unwrap();

        let mut reader = super::GptStreamReader::<_, 512>::new(&mut io, sector).unwrap();

        // bounds OK (align + limits) but overlaps must fail
        reader.validate_bounds().unwrap();
        assert!(reader.validate_overlaps().is_err());
    }

    #[test]
    fn gpt_cursor_works_with_4k_sector() {
        let sector_size = 4096u64;
        let total_sectors = 5_000u64; // 4K * 5000 = ~20 MiB
        let mut buf = vec![0u8; (sector_size * total_sectors) as usize];
        let mut io = MemRimIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, total_sectors).unwrap();

        let p = gpt::GptEntry::new(
            guids::GPT_PARTITION_TYPE_DATA,
            [9; 16],
            1024,
            4095,
            0,
            "data",
        );
        gpt::write_gpt_from_entries_with_sector(
            &mut io,
            &[p],
            sector_size,
            total_sectors,
            [0xEF; 16],
        )
        .unwrap();

        let mut reader = super::GptStreamReader::<_, 4096>::new(&mut io, sector_size).unwrap();
        let v: Vec<_> = reader.iter().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(v.len(), 1);
        reader.validate_bounds().unwrap();
        reader.validate_overlaps().unwrap();
    }

    #[test]
    fn gpt_stream_writer_roundtrip_crc_ok() {
        let sector = 512u64;
        let total = 20_000u64;
        let mut buf = vec![0u8; (sector * total) as usize];
        let mut io = MemRimIO::new(&mut buf);

        // MBR protectif
        mbr::write_mbr_protective(&mut io, total).unwrap();

        // 2 partitions simples
        let p1 = gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_ESP, [1; 16], 2048, 4095, 0, "ESP");
        let p2 = gpt::GptEntry::new(
            guids::GPT_PARTITION_TYPE_LINUX,
            [2; 16],
            4096,
            9999,
            0,
            "root",
        );

        {
            // N doit couvrir max(sector_size, entry_size)
            let mut w =
                GptStreamWriter::<_, 4096>::new(&mut io, sector, total, [0xAB; 16]).unwrap();
            w.write_entries(2, [p1, p2].into_iter()).unwrap();
            w.finalize().unwrap();
        }

        // Relire via le curseur no-alloc et valider
        let mut reader = super::GptStreamReader::<_, 4096>::new(&mut io, sector).unwrap();
        let parts: Vec<_> = reader.iter().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].start_lba, 2048);
        assert_eq!(parts[1].end_lba, 9999);

        // bornes, overlaps, CRC
        reader.validate_bounds().unwrap();
        reader.validate_overlaps().unwrap();
        reader.validate_crc().unwrap();
    }

    #[test]
    fn gpt_stream_writer_backup_mirrors_primary_entries() {
        let sector = 512u64;
        let total = 32_768u64;
        let mut buf = vec![0u8; (sector * total) as usize];
        let mut io = MemRimIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, total).unwrap();

        let parts = [
            gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_DATA, [9; 16], 2048, 4095, 0, "A"),
            gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_DATA, [8; 16], 8192, 9999, 0, "B"),
        ];

        {
            let mut w =
                GptStreamWriter::<_, 4096>::new(&mut io, sector, total, [0xCD; 16]).unwrap();
            w.write_entries(parts.len(), parts.into_iter()).unwrap();
            w.finalize().unwrap();
        }

        // Read primary & backup headers to identify zones
        let hdr_primary: gpt::GptHeader = io
            .read_struct_lba(gpt::GPT_PRIMARY_HEADER_LBA, sector)
            .unwrap();
        let hdr_backup: gpt::GptHeader =
            io.read_struct_lba(hdr_primary.backup_lba, sector).unwrap();

        assert_eq!(hdr_primary.entries_crc32, hdr_backup.entries_crc32);

        // Compare primary vs backup entries region, sector by sector (no large allocations)
        let ss = sector as usize;
        let mut a = [0u8; 4096];
        let mut b = [0u8; 4096];
        assert!(ss <= a.len());

        // entry table size in bytes
        let table_bytes = (hdr_primary.num_entries as usize) * (hdr_primary.entry_size as usize);
        let sectors_for_table = table_bytes.div_ceil(ss);

        for i in 0..sectors_for_table {
            io.read_at_lba(hdr_primary.entries_lba + i as u64, sector, &mut a[..ss])
                .unwrap();
            io.read_at_lba(hdr_backup.entries_lba + i as u64, sector, &mut b[..ss])
                .unwrap();
            assert_eq!(&a[..ss], &b[..ss], "entries sector {i} differs");
        }
    }

    #[test]
    fn gpt_stream_writer_rejects_entry_size_exceeds_sector() {
        let sector = 512u64;
        let total = 10_000u64;
        let mut buf = vec![0u8; (sector * total) as usize];
        let mut io = MemRimIO::new(&mut buf);

        // Construire un header valide puis forcer une entry_size aberrante
        let mut hdr = gpt::GptHeader::new(sector, total, [0xEE; 16]).unwrap();
        hdr.entry_size = 1024; // > sector

        // from_header doit refuser (per_sector == 0)
        let err = GptStreamWriter::<_, 2048>::from_header(&mut io, sector, hdr).unwrap_err();
        match err {
            PartError::Gpt(GptError::EntrySizeExceedsSector {
                entry_size,
                sector_size,
            }) => {
                assert_eq!(entry_size, 1024);
                assert_eq!(sector_size, 512);
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn gpt_stream_writer_works_with_4k_sector() {
        let sector = 4096u64;
        let total = 5_000u64;
        let mut buf = vec![0u8; (sector * total) as usize];
        let mut io = MemRimIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, total).unwrap();

        let p = gpt::GptEntry::new(
            guids::GPT_PARTITION_TYPE_DATA,
            [3; 16],
            1024,
            4095,
            0,
            "data",
        );

        {
            // N = 4096 couvre secteur + entry (128)
            let mut w =
                GptStreamWriter::<_, 4096>::new(&mut io, sector, total, [0xEF; 16]).unwrap();
            w.write_entries(1, core::iter::once(p)).unwrap();
            w.finalize().unwrap();
        }

        let mut reader = super::GptStreamReader::<_, 4096>::new(&mut io, sector).unwrap();
        let v: Vec<_> = reader.iter().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].start_lba, 1024);
        assert_eq!(v[0].end_lba, 4095);
        reader.validate_bounds().unwrap();
        reader.validate_overlaps().unwrap();
        reader.validate_crc().unwrap();
    }
}
