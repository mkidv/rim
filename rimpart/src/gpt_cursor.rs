// SPDX-License-Identifier: MIT
#![allow(dead_code)]

use crate::errors::*;
use crate::gpt::{GPT_PRIMARY_HEADER_LBA, GPT_SIGNATURE, GptEntry, GptHeader, align_lba_1m};
use crate::io_ext::BlockIOLbaExt;
use rimio::prelude::*;
use zerocopy::FromBytes;

/// Curseur GPT sans allocation: lit les entrées une par une avec un buffer pile.
#[derive(Debug)]
pub struct GptCursor<'io, IO: BlockIO + ?Sized, const N: usize> {
    io: &'io mut IO,
    hdr: GptHeader,
    sector_size: u64,
    entry_sz: usize,
    entry_buf: [u8; N],
    sector_buf: [u8; N],
    cached_lba: Option<u64>,
}

impl<'io, IO: BlockIO + ?Sized, const N: usize> GptCursor<'io, IO, N> {
    /// Construction: lit le header primaire, vérifie tailles.
    pub fn new(io: &'io mut IO, sector_size: u64) -> PartResult<Self> {
        let hdr: GptHeader = io.read_struct_lba(GPT_PRIMARY_HEADER_LBA, sector_size)?;
        if &hdr.signature != GPT_SIGNATURE {
            return Err(PartError::Invalid("GPT: invalid signature"));
        }
        let entry_sz = hdr.entry_size as usize;
        let base = core::mem::size_of::<GptEntry>();
        if entry_sz < base || (entry_sz % 8) != 0 {
            return Err(PartError::Invalid("GPT: invalid entry_size"));
        }
        if entry_sz > N {
            return Err(PartError::Other("GPT: entry_size exceeds stack buffer"));
        }
        Ok(Self {
            io,
            hdr,
            sector_size,
            entry_sz,
            entry_buf: [0u8; N],
            sector_buf: [0u8; N],
            cached_lba: None,
        })
    }

    #[inline]
    pub fn header(&self) -> &GptHeader {
        &self.hdr
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.hdr.num_entries as usize
    }

    /// Itérer via callback (ignore les slots vides).
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

    /// Recherche la première entrée validant un prédicat.
    pub fn find_first<F>(&mut self, mut pred: F) -> PartResult<Option<(usize, GptEntry)>>
    where
        F: FnMut(&GptEntry) -> bool,
    {
        let len = self.len();
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
        let align = align_lba_1m(self.sector_size);
        let len = self.len();
        for i in 0..len {
            let e = self.read_at(i)?;
            if e.is_empty() {
                continue;
            }
            e.validate_basic()?;
            e.validate_in_bounds(self.hdr.first_usable_lba, self.hdr.last_usable_lba, align)?;
        }
        Ok(())
    }

    /// Détection d’overlaps O(n²) en streaming (pas de Vec).
    pub fn validate_overlaps(&mut self) -> PartResult<()> {
        let n = self.len();
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
                if ei.start_lba <= ej.end_lba && ej.start_lba <= ei.end_lba {
                    return Err(PartError::Other("GPT: partition overlap detected"));
                }
            }
        }
        Ok(())
    }

    /// Lecture d’une entrée arbitraire (copie vers buffer interne).
    fn read_at(&mut self, index: usize) -> PartResult<GptEntry> {
        let off = index as u64 * self.entry_sz as u64;
        let base_lba = self.hdr.entries_lba + (off / self.sector_size);
        let in_sector = (off % self.sector_size) as usize;
        let entry_size = self.entry_sz;
        let ss = self.sector_size as usize;

        if entry_size > N || ss > N {
            return Err(PartError::Other("GPT: entry/sector exceeds stack buffer"));
        }

        // Cas 1 : l'entrée tient dans un seul secteur
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
            // Cas 2 : chevauchement sur 2 secteurs
            // Lire premier secteur (avec cache)
            if self.cached_lba != Some(base_lba) {
                self.io
                    .read_at_lba(base_lba, self.sector_size, &mut self.sector_buf[..ss])?;
                self.cached_lba = Some(base_lba);
            }
            let first = ss - in_sector;
            self.entry_buf[..first].copy_from_slice(&self.sector_buf[in_sector..ss]);

            // Lire second secteur (pas le même LBA, on remplace le cache)
            let next_lba = base_lba + 1;
            self.io
                .read_at_lba(next_lba, self.sector_size, &mut self.sector_buf[..ss])?;
            self.cached_lba = Some(next_lba);
            self.entry_buf[first..entry_size]
                .copy_from_slice(&self.sector_buf[..(entry_size - first)]);
        }

        let base = core::mem::size_of::<GptEntry>();
        let e = GptEntry::ref_from_bytes(&self.entry_buf[..base])
            .map_err(|_| PartError::Invalid("GPT: invalid entry"))?;
        Ok(*e)
    }

    pub fn iter<'c>(&'c mut self) -> GptIter<'c, 'io, IO, N> {
        GptIter::new(self, 0, self.len())
    }
}

// ── Iterator pour GptCursor ───────────────────────────────────────────────────

// Itérateur avec deux lifetimes distincts
pub struct GptIter<'c, 'io, IO: BlockIO + ?Sized, const N: usize> {
    cursor: &'c mut GptCursor<'io, IO, N>,
    pos: usize,
    len: usize,
}

impl<'c, 'io, IO: BlockIO + ?Sized, const N: usize> GptIter<'c, 'io, IO, N> {
    pub fn new(cursor: &'c mut GptCursor<'io, IO, N>, pos: usize, len: usize) -> Self {
        Self { cursor, pos, len }
    }
}

// Impl de Iterator : utilise bien les deux lifetimes
impl<'c, 'io, IO: BlockIO + ?Sized, const N: usize> Iterator for GptIter<'c, 'io, IO, N> {
    type Item = PartResult<GptEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.len {
            let i = self.pos;
            self.pos += 1;
            match self.cursor.read_at(i) {
                Ok(e) if !e.is_empty() => return Some(Ok(e)),
                Ok(_) => continue, // skip empty
                Err(e) => return Some(Err(e)),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::{gpt, guids, mbr};
    use rimio::prelude::MemBlockIO;

    #[test]
    fn gpt_cursor_iter_and_find_first_esp() {
        let sector = 512u64;
        let total_sectors = 20_000u64;
        let mut buf = vec![0u8; (sector * total_sectors) as usize];
        let mut io = MemBlockIO::new(&mut buf);

        // MBR protectif + GPT avec 2 partitions alignées
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
        gpt::write_gpt(&mut io, &[p1, p2], total_sectors, [0xAB; 16]).unwrap();

        // Cursor (stack 512)
        let mut cur = super::GptCursor::<_, 512>::new(&mut io, sector).unwrap();

        {
            // iter() ignore les slots vides → on doit voir 2 entrées
            let parts: Vec<_> = cur.iter().collect::<Result<Vec<_>, _>>().unwrap();
            assert_eq!(parts.len(), 2);
            assert_eq!(parts[0].start_lba, 2048);
            assert_eq!(parts[1].start_lba, 4096);
        }

        // find_first ESP
        let (idx, esp) = cur
            .find_first(|e| e.kind() == guids::GptPartitionKind::Esp)
            .unwrap()
            .expect("ESP not found");
        assert_eq!(idx, 0);
        assert_eq!(esp.end_lba, 4095);

        // validations
        cur.validate_bounds().unwrap();
        cur.validate_overlaps().unwrap();
    }

    #[test]
    fn gpt_cursor_detects_overlap() {
        let sector = 512u64;
        let total_sectors = 20_000u64;
        let mut buf = vec![0u8; (sector * total_sectors) as usize];
        let mut io = MemBlockIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, total_sectors).unwrap();

        // 2 partitions qui se chevauchent
        let p1 = gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_DATA, [3; 16], 2048, 6000, 0, "A");
        let p2 = gpt::GptEntry::new(guids::GPT_PARTITION_TYPE_DATA, [4; 16], 4096, 7000, 0, "B");
        gpt::write_gpt(&mut io, &[p1, p2], total_sectors, [0xCD; 16]).unwrap();

        let mut cur = super::GptCursor::<_, 512>::new(&mut io, sector).unwrap();

        // bornes OK (align + limites) mais overlaps doivent échouer
        cur.validate_bounds().unwrap();
        assert!(cur.validate_overlaps().is_err());
    }

    #[test]
    fn gpt_cursor_works_with_4k_sector() {
        let sector = 4096u64;
        let total_sectors = 5_000u64; // 4K * 5000 = ~20 MiB
        let mut buf = vec![0u8; (sector * total_sectors) as usize];
        let mut io = MemBlockIO::new(&mut buf);

        mbr::write_mbr_protective(&mut io, total_sectors).unwrap();

        let p = gpt::GptEntry::new(
            guids::GPT_PARTITION_TYPE_DATA,
            [9; 16],
            1024,
            4095,
            0,
            "data",
        );
        gpt::write_gpt_with_sector(&mut io, &[p], total_sectors, [0xEF; 16], sector).unwrap();

        let mut cur = super::GptCursor::<_, 4096>::new(&mut io, sector).unwrap();
        let v: Vec<_> = cur.iter().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(v.len(), 1);
        cur.validate_bounds().unwrap();
        cur.validate_overlaps().unwrap();
    }
}
