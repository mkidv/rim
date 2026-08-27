// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::builder::NtfsIndexEntry;
use crate::meta::NtfsMeta;
use crate::types::{IndexEntryFlags, IndexEntryHeader, NtfsIndexRecord};
use zerocopy::IntoBytes;

use rimio::prelude::RimIOResult;

pub struct IndexTreeLayout {
    pub root_entries: Vec<u8>,
    pub allocation_blocks: Vec<Vec<u8>>, // In a real streaming scenario, this could be a generator
    pub bitmap: Vec<u8>,
    pub total_clusters: u64,
}

pub struct IndexTreeBuilder;

impl IndexTreeBuilder {
    pub fn build(meta: &NtfsMeta, entries: Vec<NtfsIndexEntry>) -> RimIOResult<IndexTreeLayout> {
        let index_record_size = meta.index_record_size as usize;
        // Max payload in an Index Record (4KB usually)
        // Header (Indx + USA) ~ 24 + 2*2 = 28 bytes
        // Node Header = 16 bytes
        // End Entry = 16 bytes
        // Safety margin = 32 bytes
        let max_payload = index_record_size.saturating_sub(64 + 32);

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
            let mut io = rimio::prelude::MemRimIO::new(&mut buf);
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
        let bitmap_len = blocks.len().div_ceil(8);
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
