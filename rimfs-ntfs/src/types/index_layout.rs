// SPDX-License-Identifier: MIT
//! NTFS B-Tree Index Layout Builder
//!
//! Partitions directory index entries into index allocation blocks ($I30)
//! and generates the root index structure, allocation blocks, and bitmap.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::meta::NtfsMeta;
use crate::system::upcase::UpcaseHandle;
use crate::types::{IndexEntryFlags, IndexEntryHeader, NtfsIndexEntry, NtfsIndexRecord};
use crate::utils::compare_names_upcase;
use zerocopy::IntoBytes;

use rimio::prelude::*;

pub struct IndexTreeLayout {
    pub root_entries: Vec<u8>,
    pub allocation_blocks: Vec<Vec<u8>>,
    pub bitmap: Vec<u8>,
    pub total_clusters: u64,
}

impl IndexTreeLayout {
    /// Writes all allocation blocks sequentially into the allocated disk runlist.
    pub fn write_allocation_blocks<IO: RimIO + ?Sized>(
        &self,
        io: &mut IO,
        meta: &NtfsMeta,
        runs: &rimio::run::RunList,
    ) -> RimIOResult {
        let mut mapped = MappedRimIO::new(io, runs, meta.bytes_per_cluster as usize);
        let mut off = 0u64;
        for block in &self.allocation_blocks {
            mapped.write_at(off, block)?;
            off += meta.index_record_size as u64;
        }
        Ok(())
    }
}

pub enum DirectoryIndexResult {
    /// Resident index root entries (small directory fitting in MFT record)
    Resident { entries_buf: Vec<u8> },
    /// Non-resident B-tree layout with allocation blocks and bitmap
    NonResident { layout: IndexTreeLayout },
}

pub struct IndexTreeBuilder;

impl IndexTreeBuilder {
    /// Build directory index: either resident buffer or non-resident tree layout.
    ///
    /// Automatically sorts entries according to NTFS UpCase collation rules
    /// and formats the terminator / subnodes as appropriate.
    pub fn build_directory_index(
        meta: &NtfsMeta,
        mut entries: Vec<NtfsIndexEntry>,
    ) -> RimIOResult<DirectoryIndexResult> {
        let upcase = UpcaseHandle::from_flavor(&meta.upcase_flavor);
        entries.sort_by(|a, b| compare_names_upcase(&a.name, &b.name, &upcase));

        let entries_len: usize = entries.iter().map(|e| e.len()).sum();
        let resident_max = meta.mft_record_size as usize / 3;

        if entries_len < resident_max {
            let mut buf = Vec::new();
            for e in &entries {
                let len = e.len();
                let old = buf.len();
                buf.resize(old + len, 0);
                let mut mem_io = MemRimIO::new(&mut buf[old..]);
                e.write_to_io(&mut mem_io, 0)?;
            }
            let last = IndexEntryHeader::new(0, 0, true);
            buf.extend_from_slice(last.as_bytes());
            Ok(DirectoryIndexResult::Resident { entries_buf: buf })
        } else {
            let layout = Self::build(meta, entries)?;
            Ok(DirectoryIndexResult::NonResident { layout })
        }
    }

    pub fn build(meta: &NtfsMeta, entries: Vec<NtfsIndexEntry>) -> RimIOResult<IndexTreeLayout> {
        let index_record_size = meta.index_record_size as usize;
        // Max payload in an Index Record (4KB usually)
        // Header (Indx + USA + padding) = 88 bytes
        // Node Header = 16 bytes
        // End Entry = 16 bytes
        // Safety margin = 32 bytes
        let max_payload = index_record_size.saturating_sub(88 + 32);

        let mut blocks: Vec<Vec<NtfsIndexEntry>> = Vec::new();
        let mut root_entries_indices: Vec<(usize, u64)> = Vec::new(); // (index in entries, vcn)

        let mut current_block_entries = Vec::new();
        let mut current_len = 0;

        // Partition entries into blocks
        for (i, entry) in entries.iter().enumerate() {
            let entry_len = entry.len();

            // Check if adding this entry + End Entry (16) overflows the block
            if current_len + entry_len + 16 > max_payload {
                let block_index = blocks.len() as u64;
                let vcn = meta.index_block_to_vcn(block_index);
                blocks.push(current_block_entries);

                // Promote current entry as pivot to the Root with child VCN
                root_entries_indices.push((i, vcn));

                // Reset for next block
                current_block_entries = Vec::new();
                current_len = 0;
            } else {
                current_block_entries.push(entry.clone());
                current_len += entry_len;
            }
        }

        // Remaining entries go to last block
        let last_block_index = blocks.len() as u64;
        let last_block_vcn = meta.index_block_to_vcn(last_block_index);
        blocks.push(current_block_entries);

        // --- 1. Build Allocation Blocks (Raw Bytes) ---
        let total_clusters = meta.total_clusters_for_index_blocks(blocks.len());

        let mut allocation_blocks = Vec::with_capacity(blocks.len());

        for (i, block_entries) in blocks.iter().enumerate() {
            let vcn = meta.index_block_to_vcn(i as u64);
            let mut record = NtfsIndexRecord::new(vcn, false);
            for entry in block_entries {
                record.add_entry(entry.clone());
            }
            allocation_blocks.push(record.to_raw_buffer(meta)?);
        }

        // --- 2. Build Root Entries (Raw Bytes) ---
        let mut root_entries_bytes = Vec::new();

        // Add pivot entries
        for (entry_idx, vcn) in root_entries_indices {
            let entry = &entries[entry_idx];
            let mut new_entry = entry.clone();
            new_entry.vcn = Some(vcn);
            new_entry.flags |= IndexEntryFlags::HAS_SUBNODES;

            // Serialize
            let mut buf = vec![0u8; new_entry.len()];
            let mut io = MemRimIO::new(&mut buf);
            new_entry.write_to_io(&mut io, 0)?;
            root_entries_bytes.extend_from_slice(&buf);
        }

        // Add End Entry to Root (points to the last block)
        let mut last_entry = IndexEntryHeader::new(0, 0, true); // LAST_ENTRY
        last_entry.flags |= IndexEntryFlags::HAS_SUBNODES.bits();
        last_entry.entry_length += 8; // +8 bytes for VCN

        let mut last_bytes = Vec::from(last_entry.as_bytes());
        last_bytes.extend_from_slice(&last_block_vcn.to_le_bytes());
        root_entries_bytes.extend_from_slice(&last_bytes);

        // --- 3. Build Bitmap ---
        // Windows/mkntfs stores the $I30 bitmap with a minimum size of 8 bytes.
        let bitmap_len = blocks.len().div_ceil(8).max(8);
        let mut bitmap = vec![0u8; bitmap_len];

        for i in 0..blocks.len() {
            bitmap[i / 8] |= 1 << (i % 8);
        }

        Ok(IndexTreeLayout {
            root_entries: root_entries_bytes,
            allocation_blocks,
            bitmap,
            total_clusters,
        })
    }
}
