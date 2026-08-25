// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::meta::FsMeta;
use rimio::prelude::*;

/// Trait for cluster-based filesystems (FAT32, ExFAT, etc.)
pub trait FatFsMeta: FsMeta<u32> {
    const FIRST_CLUSTER: u32;

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
}

/// A unified, buffered view of the FAT table.
/// Handles extraction, insertion, and sector-level caching (1024-byte window).
#[derive(Debug)]
pub struct FatDriver<'a, M: FatFsMeta> {
    pub meta: &'a M,
    pub buffer: [u8; 1024],
    pub sector_idx: u64, // Start sector of the buffer
    pub valid_len: usize,
    pub valid: bool,
    pub dirty: bool,
}

impl<'a, M: FatFsMeta> FatDriver<'a, M> {
    pub fn new(meta: &'a M) -> Self {
        Self {
            meta,
            buffer: [0u8; 1024],
            sector_idx: 0,
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

    /// Ensure the required range is in the buffer (always loads from FAT 0).
    fn ensure_loaded<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        byte_offset: u64,
        len: usize,
    ) -> RimIOResult {
        let sector_idx = byte_offset / 512;
        let end_sector = (byte_offset + len as u64 - 1) / 512;

        if self.valid && sector_idx >= self.sector_idx && end_sector < self.sector_idx + 2 {
            return Ok(());
        }

        self.flush(io)?;

        let table_off = self.meta.fat_table_offset(0);
        let fat_size = self.meta.fat_size_bytes();
        let disk_offset = sector_idx * 512;

        let remaining = fat_size.saturating_sub(disk_offset);
        let to_read = core::cmp::min(1024, remaining as usize);

        io.read_at(table_off + disk_offset, &mut self.buffer[..to_read])?;
        self.sector_idx = sector_idx;
        self.valid_len = to_read;
        self.valid = true;
        Ok(())
    }

    /// Read an entry from the FAT.
    pub fn get<IO: RimIO + ?Sized>(&mut self, io: &mut IO, cluster: u32) -> RimIOResult<u32> {
        let bits = self.meta.bits_per_entry();
        let bit_offset = cluster as u64 * bits as u64;
        let byte_offset_in_fat = bit_offset / 8;
        let bit_shift = (bit_offset % 8) as u8;
        let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;

        self.ensure_loaded(io, byte_offset_in_fat, bytes_needed)?;

        let off_in_buf = (byte_offset_in_fat - self.sector_idx * 512) as usize;
        let val = extract_entry_from_buf(
            &self.buffer[off_in_buf..],
            bits,
            bit_shift,
            self.meta.entry_mask(),
        );
        Ok(val)
    }

    /// Write an entry to the FAT. Mirroring is handled on flush.
    pub fn set<IO: RimIO + ?Sized>(
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
        );
        self.dirty = true;
        Ok(())
    }

    /// Read an entry from the FAT.
    /// Replacement for `read_fat_entry`.
    pub fn read_entry<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        cluster: u32,
    ) -> RimIOResult<u32> {
        self.get(io, cluster)
    }

    /// Write an entry to the FAT. Mirroring is handled on flush.
    /// Replacement for `write_fat_entry`.
    pub fn write_entry<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        cluster: u32,
        value: u32,
    ) -> RimIOResult {
        self.set(io, cluster, value)?;
        // Auto-flush to maintain previous behavior of write_fat_entry which flushed immediately
        self.flush(io)
    }

    /// Read an entire chain into a vector of clusters.
    /// Replacement for `read_fat_chain`.
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
            current = self.get(io, current)?;
        }
        Ok(chain)
    }

    /// Write a chain of clusters to the FAT. Handles mirroring automatically.
    /// Replacement for `write_fat_chain`.
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

            self.set(io, current, next)?;
        }
        self.flush(io)?;
        Ok(())
    }

    /// Write a chain of clusters from a RunList to the FAT. Handles mirroring automatically.
    /// Replacement for `write_chain_run_list`.
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
                    self.set(io, p, current)?;
                }
                prev = Some(current);
            }
        }

        if let Some(p) = prev {
            let eoc = self.meta.entry_mask();
            self.set(io, p, eoc)?;
        }

        self.flush(io)?;
        Ok(())
    }

    /// Efficiently scan the entire FAT to count free clusters.
    /// Replacement for `scan_free_clusters`.
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
                    let val = self.get(io, cluster)?;
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
}

#[inline]
fn extract_entry_from_buf(buf: &[u8], bits: u32, bit_shift: u8, mask: u32) -> u32 {
    let mut raw = 0u64;
    let bytes_needed = (bits + bit_shift as u32).div_ceil(8) as usize;
    for (i, &b) in buf.iter().enumerate().take(bytes_needed) {
        raw |= (b as u64) << (i * 8);
    }
    let val = (raw >> bit_shift) & ((1u64 << bits) - 1);
    (val as u32) & mask
}

#[inline]
fn insert_entry_into_buf(buf: &mut [u8], bits: u32, bit_shift: u8, value: u32) {
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
