// SPDX-License-Identifier: MIT

//! Common FAT cluster traits and cluster mask definitions.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::meta::FsMeta;
use rimio::prelude::*;

/// Trait for cluster-based filesystems (FAT32, ExFAT, etc.)
pub trait FatFsMeta: FsMeta<u32> {
    const FIRST_CLUSTER: u32 = 2;

    /// Number of bits per FAT entry (e.g., 12, 16, 32).
    /// Used for calculating offsets in the FAT.
    fn bits_per_entry(&self) -> u32;

    /// The mask to apply to read values (e.g., 0x0FFFFFFF for FAT32).
    fn entry_mask(&self) -> u32;

    /// The starting offset of the Nth FAT table in bytes.
    fn fat_table_offset(&self, fat_index: u8) -> u64;

    /// The size of a single FAT table in bytes.
    fn fat_size_bytes(&self) -> u64;

    /// Checks if a cluster is End-of-Chain
    fn is_eoc(&self, cluster: u32) -> bool;

    fn num_fats(&self) -> u8;

    fn media_descriptor(&self) -> u8 {
        0xF8
    }

    #[inline]
    fn reserved_entry_0(&self) -> u32 {
        (self.entry_mask() & !0xFF) | self.media_descriptor() as u32
    }

    #[inline]
    fn reserved_entry_1(&self) -> u32 {
        self.entry_mask()
    }
}

/// A unified, buffered view of the FAT table.
/// Handles extraction, insertion, and sector-level caching (4096-byte window).
#[derive(Debug)]
pub struct FatDriver<'a, M: FatFsMeta> {
    pub meta: &'a M,
    pub buffer: [u8; 4096],
    pub sector_idx: u64, // Start sector of the buffer
    pub fat_index: u8,
    pub valid_len: usize,
    pub valid: bool,
    pub dirty: bool,
}

impl<'a, M: FatFsMeta> FatDriver<'a, M> {
    pub fn new(meta: &'a M) -> Self {
        Self {
            meta,
            buffer: [0u8; 4096],
            sector_idx: 0,
            fat_index: 0,
            valid_len: 0,
            valid: false,
            dirty: false,
        }
    }

    /// Flush the buffer if dirty. Writes to ALL FAT tables (mirroring).
    pub fn flush<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> RimIOResult {
        if self.valid && self.dirty {
            for fi in 0..self.meta.num_fats() {
                let table_off = self.meta.fat_table_offset(fi);
                io.write_at(
                    table_off + self.sector_idx * 512,
                    &self.buffer[..self.valid_len],
                )?;
            }
            self.dirty = false;
        }
        Ok(())
    }

    /// Ensure the required range is in the buffer using a read-only stream.
    fn ensure_loaded_ro<IO: RimRead + ?Sized>(
        &mut self,
        io: &mut IO,
        byte_offset: u64,
        len: usize,
    ) -> RimIOResult {
        self.ensure_loaded_ro_from_table(io, 0, byte_offset, len)
    }

    /// Ensure the required range is in the buffer from a specific FAT copy.
    fn ensure_loaded_ro_from_table<IO: RimRead + ?Sized>(
        &mut self,
        io: &mut IO,
        fat_index: u8,
        byte_offset: u64,
        len: usize,
    ) -> RimIOResult {
        let sector_idx = byte_offset / 512;
        let end_sector = (byte_offset + len as u64 - 1) / 512;
        let sectors_per_win = (self.buffer.len() / 512) as u64;

        if self.valid
            && self.fat_index == fat_index
            && sector_idx >= self.sector_idx
            && end_sector < self.sector_idx + sectors_per_win
        {
            return Ok(());
        }

        if self.valid && self.dirty {
            return Err(RimIOError::Invalid("State"));
        }

        let table_off = self.meta.fat_table_offset(fat_index);
        let fat_size = self.meta.fat_size_bytes();
        let disk_offset = sector_idx * 512;

        let remaining = fat_size.saturating_sub(disk_offset);
        let to_read = core::cmp::min(self.buffer.len(), remaining as usize);

        io.read_at(table_off + disk_offset, &mut self.buffer[..to_read])?;
        self.sector_idx = sector_idx;
        self.fat_index = fat_index;
        self.valid_len = to_read;
        self.valid = true;
        Ok(())
    }

    /// Read an entry from the FAT using a read-only stream.
    pub fn read_entry_ro<IO: RimRead + ?Sized>(
        &mut self,
        io: &mut IO,
        cluster: u32,
    ) -> RimIOResult<u32> {
        self.read_entry_ro_from_table(io, 0, cluster)
    }

    /// Read an entry from a specific FAT copy using a read-only stream.
    pub fn read_entry_ro_from_table<IO: RimRead + ?Sized>(
        &mut self,
        io: &mut IO,
        fat_index: u8,
        cluster: u32,
    ) -> RimIOResult<u32> {
        let bits = self.meta.bits_per_entry();
        let bit_offset = cluster as u64 * bits as u64;
        let byte_offset_in_fat = bit_offset / 8;
        let bit_shift = (bit_offset % 8) as u8;
        let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;

        self.ensure_loaded_ro_from_table(io, fat_index, byte_offset_in_fat, bytes_needed)?;

        let off_in_buf = (byte_offset_in_fat - self.sector_idx * 512) as usize;
        let val = extract_entry_from_buf(
            &self.buffer[off_in_buf..],
            bits,
            bit_shift,
            self.meta.entry_mask(),
        );
        Ok(val)
    }

    /// Ensure the required range is in the buffer (always loads from FAT 0).
    fn ensure_loaded<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        byte_offset: u64,
        len: usize,
    ) -> RimIOResult {
        let sector_idx = byte_offset / 512;
        let end_sector = (byte_offset + len as u64 - 1) / 512;
        let sectors_per_win = (self.buffer.len() / 512) as u64;

        if self.valid
            && self.fat_index == 0
            && sector_idx >= self.sector_idx
            && end_sector < self.sector_idx + sectors_per_win
        {
            return Ok(());
        }

        self.flush(io)?;
        self.ensure_loaded_ro(io, byte_offset, len)
    }

    /// Read an entry from the FAT.
    pub fn read_entry<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        cluster: u32,
    ) -> RimIOResult<u32> {
        let bits = self.meta.bits_per_entry();
        let bit_offset = cluster as u64 * bits as u64;
        let byte_offset_in_fat = bit_offset / 8;
        let bit_shift = (bit_offset % 8) as u8;
        let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;

        let sector_idx = byte_offset_in_fat / 512;
        let end_sector = (byte_offset_in_fat + bytes_needed as u64 - 1) / 512;
        let sectors_per_win = (self.buffer.len() / 512) as u64;

        if self.valid
            && self.fat_index == 0
            && sector_idx >= self.sector_idx
            && end_sector < self.sector_idx + sectors_per_win
        {
            let off_in_buf = (byte_offset_in_fat - self.sector_idx * 512) as usize;
            let val = extract_entry_from_buf(
                &self.buffer[off_in_buf..],
                bits,
                bit_shift,
                self.meta.entry_mask(),
            );
            return Ok(val);
        }

        if self.valid && self.dirty {
            self.flush(io)?;
        }
        self.read_entry_ro(io, cluster)
    }

    /// Read an entry from a specific FAT copy without mirroring.
    pub fn read_entry_from_table<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        fat_index: u8,
        cluster: u32,
    ) -> RimIOResult<u32> {
        let bits = self.meta.bits_per_entry();
        let bit_offset = cluster as u64 * bits as u64;
        let byte_offset_in_fat = bit_offset / 8;
        let bit_shift = (bit_offset % 8) as u8;
        let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;

        let sector_idx = byte_offset_in_fat / 512;
        let end_sector = (byte_offset_in_fat + bytes_needed as u64 - 1) / 512;
        let sectors_per_win = (self.buffer.len() / 512) as u64;

        if self.valid
            && self.fat_index == fat_index
            && sector_idx >= self.sector_idx
            && end_sector < self.sector_idx + sectors_per_win
        {
            let off_in_buf = (byte_offset_in_fat - self.sector_idx * 512) as usize;
            let val = extract_entry_from_buf(
                &self.buffer[off_in_buf..],
                bits,
                bit_shift,
                self.meta.entry_mask(),
            );
            return Ok(val);
        }

        if self.valid && self.dirty {
            self.flush(io)?;
        }
        self.read_entry_ro_from_table(io, fat_index, cluster)
    }

    /// Write an entry to the FAT. Mirroring is handled on flush.
    pub fn write_entry<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        cluster: u32,
        value: u32,
    ) -> RimIOResult {
        let bits = self.meta.bits_per_entry();
        let bit_offset = cluster as u64 * bits as u64;
        let byte_offset_in_fat = bit_offset / 8;
        let bit_shift = (bit_offset % 8) as u8;
        let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;

        self.ensure_loaded(io, byte_offset_in_fat, bytes_needed)?;

        let off_in_buf = (byte_offset_in_fat - self.sector_idx * 512) as usize;
        insert_entry_into_buf(
            &mut self.buffer[off_in_buf..off_in_buf + bytes_needed],
            bits,
            bit_shift,
            value,
            self.meta.entry_mask(),
        );
        self.dirty = true;
        Ok(())
    }

    /// Read an entire chain into a vector of clusters.
    pub fn read_chain<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        start_cluster: u32,
    ) -> RimIOResult<Vec<u32>> {
        let mut chain = Vec::new();
        let mut current = start_cluster;
        const MAX_CHAIN_LEN: usize = 1_000_000;

        while !self.meta.is_eoc(current) {
            chain.push(current);
            if chain.len() >= MAX_CHAIN_LEN {
                break;
            }
            current = self.read_entry(io, current)?;
        }
        Ok(chain)
    }

    /// Write a chain of clusters to the FAT. Handles mirroring automatically.
    pub fn write_chain<IO: RimIO + ?Sized>(&mut self, io: &mut IO, chain: &[u32]) -> RimIOResult {
        if chain.is_empty() {
            return Ok(());
        }

        let eoc_val = self.meta.entry_mask();

        for (i, &current) in chain.iter().enumerate() {
            let next = if i + 1 < chain.len() {
                chain[i + 1]
            } else {
                eoc_val
            };

            self.write_entry(io, current, next)?;
        }
        self.flush(io)?;
        Ok(())
    }

    /// Write a chain of clusters from a RunList to the FAT. Handles mirroring automatically.
    pub fn write_run_list<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        chain: &RunList,
    ) -> RimIOResult {
        let mut prev: Option<u32> = None;

        for run in chain.iter() {
            for i in 0..run.length {
                let current = (run.start + i) as u32;
                if let Some(p) = prev {
                    self.write_entry(io, p, current)?;
                }
                prev = Some(current);
            }
        }

        if let Some(p) = prev {
            let eoc = self.meta.entry_mask();
            self.write_entry(io, p, eoc)?;
        }

        self.flush(io)?;
        Ok(())
    }

    /// Efficiently scan the entire FAT to count free clusters.
    pub fn find_next_free<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> RimIOResult<(usize, u32)> {
        let start = self.meta.first_data_unit();
        let end = self.meta.last_data_unit();
        let bits = self.meta.bits_per_entry();
        let entry_mask = self.meta.entry_mask();
        let table_off = self.meta.fat_table_offset(0);

        let mut free_count = 0;
        let mut first_free = None;

        match bits {
            32 => {
                let start_offset = table_off + (start as u64 * 4);
                let count = (end - start + 1) as usize;
                io.read_chunks_streamed::<4, _>(start_offset, count, 16384, |i, bytes| {
                    let val = u32::from_le_bytes(*bytes) & entry_mask;
                    if val == 0 {
                        free_count += 1;
                        if first_free.is_none() {
                            first_free = Some(start + i as u32);
                        }
                    }
                })?;
            }
            16 => {
                let start_offset = table_off + (start as u64 * 2);
                let count = (end - start + 1) as usize;
                io.read_chunks_streamed::<2, _>(start_offset, count, 32768, |i, bytes| {
                    let val = u16::from_le_bytes(*bytes) as u32 & entry_mask;
                    if val == 0 {
                        free_count += 1;
                        if first_free.is_none() {
                            first_free = Some(start + i as u32);
                        }
                    }
                })?;
            }
            _ => {
                // Fallback for FAT12: use unified FatDriver logic (self.get uses the buffer)
                for cluster in start..=end {
                    let val = self.read_entry(io, cluster)?;
                    if val == 0 {
                        free_count += 1;
                        if first_free.is_none() {
                            first_free = Some(cluster);
                        }
                    }
                }
            }
        }

        Ok((free_count, first_free.unwrap_or(end + 1)))
    }

    /// Find the next contiguous range of `count` free clusters starting from `hint`.
    pub fn find_next_free_run<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        hint: u32,
        count: u32,
    ) -> RimIOResult<Option<u32>> {
        if count == 0 {
            return Ok(Some(hint));
        }
        let start = hint.max(self.meta.first_data_unit());
        let end = self.meta.last_data_unit();

        let mut current = start;
        let mut run_start = None;
        let mut run_len = 0u32;

        while current <= end {
            let val = self.read_entry(io, current)?;
            if val == 0 {
                if run_start.is_none() {
                    run_start = Some(current);
                }
                run_len += 1;
                if run_len == count {
                    return Ok(run_start);
                }
            } else {
                run_start = None;
                run_len = 0;
            }
            current += 1;
        }

        Ok(None)
    }

    /// Find enough free clusters for `count` units, fragmented as a RunList.
    pub fn find_free_runs<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        hint: u32,
        count: u64,
    ) -> RimIOResult<Option<RunList>> {
        if count == 0 {
            return Ok(Some(RunList::new()));
        }
        let start = hint.max(self.meta.first_data_unit());
        let end = self.meta.last_data_unit();
        let wrap_limit = self.meta.first_data_unit();

        let mut list = RunList::new();
        let mut found = 0u64;
        let mut current = start;
        let total_units = u64::from(end - wrap_limit) + 1;
        let mut searched = 0u64;

        while found < count && searched < total_units {
            let val = self.read_entry(io, current)?;
            if val == 0 {
                list.push_unit(current as u64);
                found += 1;
            }
            current += 1;
            if current > end {
                current = wrap_limit;
            }
            searched += 1;
        }

        if found == count {
            Ok(Some(list))
        } else {
            Ok(None)
        }
    }

    fn fill_tables<IO: RimIO + ?Sized>(&mut self, io: &mut IO, pattern: u8) -> RimIOResult {
        self.flush(io)?;

        let fat_size = self.meta.fat_size_bytes();

        if pattern == 0 {
            for fat_index in 0..self.meta.num_fats() {
                let base = self.meta.fat_table_offset(fat_index);
                io.zero_at(base, fat_size)?;
            }
        } else {
            self.buffer.fill(pattern);

            for fat_index in 0..self.meta.num_fats() {
                let base = self.meta.fat_table_offset(fat_index);
                let mut offset = 0;

                while offset < fat_size {
                    let len = core::cmp::min(self.buffer.len() as u64, fat_size - offset) as usize;

                    io.write_at(base + offset, &self.buffer[..len])?;
                    offset += len as u64;
                }
            }
        }

        self.valid = false;
        self.dirty = false;

        Ok(())
    }

    pub fn format<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> RimIOResult {
        self.fill_tables(io, 0)?;

        // Initialize reserved FAT entries according to FAT12/16/32 semantics.
        self.write_entry(io, 0, self.meta.reserved_entry_0())?;
        self.write_entry(io, 1, self.meta.reserved_entry_1())?;

        self.flush(io)?;

        Ok(())
    }
}

#[inline]
fn extract_entry_from_buf(buf: &[u8], bits: u32, bit_shift: u8, mask: u32) -> u32 {
    match bits {
        32 if bit_shift == 0 => {
            let raw = u32::from_le_bytes(buf[..4].try_into().unwrap());
            raw & mask
        }
        16 if bit_shift == 0 => {
            let raw = u16::from_le_bytes(buf[..2].try_into().unwrap()) as u32;
            raw & mask
        }
        _ => {
            let mut raw = 0u64;
            let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;
            for (i, &b) in buf.iter().enumerate().take(bytes_needed) {
                raw |= (b as u64) << (i * 8);
            }
            let val = (raw >> bit_shift) & ((1u64 << bits) - 1);
            (val as u32) & mask
        }
    }
}

#[inline]
fn insert_entry_into_buf(buf: &mut [u8], bits: u32, bit_shift: u8, value: u32, mask: u32) {
    match bits {
        32 if bit_shift == 0 => {
            let existing = u32::from_le_bytes(buf[..4].try_into().unwrap());
            let val = (existing & !mask) | (value & mask);
            buf[..4].copy_from_slice(&val.to_le_bytes());
        }
        16 if bit_shift == 0 => {
            let existing = u16::from_le_bytes(buf[..2].try_into().unwrap()) as u32;
            let val = (existing & !mask) | (value & mask);
            buf[..2].copy_from_slice(&(val as u16).to_le_bytes());
        }
        _ => {
            let mut raw = 0u64;
            for (i, &b) in buf.iter().enumerate() {
                raw |= (b as u64) << (i * 8);
            }
            let val_mask = (1u64 << bits) - 1;
            raw &= !(val_mask << bit_shift);
            raw |= ((value as u64) & val_mask) << bit_shift;
            for (i, b) in buf.iter_mut().enumerate() {
                *b = ((raw >> (i * 8)) & 0xFF) as u8;
            }
        }
    }
}

/// Check if chain is contiguous
#[inline]
pub fn is_contiguous(chain: &[u32]) -> bool {
    if chain.is_empty() {
        return true;
    }
    let start = chain[0];
    chain
        .iter()
        .enumerate()
        .all(|(i, &c)| c == start + i as u32)
}
