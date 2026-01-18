// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::core::cursor::ClusterMeta;
use rimio::prelude::*;

/// Generic FAT-style chain helpers. Parameterized by `ClusterMeta` so it works
/// for FAT12/16/32 and exFAT (different ENTRY_SIZE/MASK/EOC).
pub mod chain {
    use super::*;

    #[inline]
    pub fn read_entry<IO, M>(io: &mut IO, meta: &M, cluster: u32, fat_index: u8) -> RimIOResult<u32>
    where
        IO: RimIO + ?Sized,
        M: ClusterMeta,
    {
        let off = meta.fat_entry_offset(cluster, fat_index);

        let mut buf = [0u8; 4];
        let n = M::ENTRY_SIZE;
        debug_assert!(n <= 4 && n != 0);

        io.read_at(off, &mut buf[..n])?;

        let mut v = 0u32;
        for (i, b) in buf.iter().enumerate() {
            v |= (*b as u32) << (8 * i);
        }

        Ok(v & M::ENTRY_MASK)
    }

    /// Read an entire chain into a vector of clusters.
    /// Stops at EOC or if loop/overflow is detected.
    pub fn read_chain<IO: RimIO + ?Sized, M: ClusterMeta>(
        io: &mut IO,
        meta: &M,
        start_cluster: u32,
    ) -> RimIOResult<Vec<u32>> {
        let mut chain = Vec::new();
        let mut current = start_cluster;

        // Safety limit (e.g. 1M clusters ~ 4GB with 4K clusters, reasonable for basic checks)
        const MAX_CHAIN_LEN: usize = 1_000_000;

        while !meta.is_eoc(current) {
            chain.push(current);
            if chain.len() >= MAX_CHAIN_LEN {
                break;
            }
            // Using index 0 for Primary FAT
            current = read_entry(io, meta, current, 0)?;
        }
        Ok(chain)
    }

    /// Build raw FAT entries for a chain: each entry points to the next,
    /// last entry points to EOC. Entries are little-endian.
    pub fn build_entries<M: ClusterMeta>(chain: &[u32]) -> Vec<u8> {
        let mut out = vec![0u8; chain.len() * M::ENTRY_SIZE];
        for (i, _) in chain.iter().enumerate() {
            let next = if i + 1 < chain.len() {
                chain[i + 1]
            } else {
                M::EOC
            };
            let val = next & M::ENTRY_MASK;
            out[i * 4..i * 4 + 4].copy_from_slice(&val.to_le_bytes());
        }
        out
    }

    /// Compute on-disk offsets for the given chain entries in the Nth FAT copy.
    pub fn entry_offsets<M: ClusterMeta>(
        meta: &impl ClusterMeta,
        chain: &[u32],
        fat_index: u8,
    ) -> Vec<u64> {
        chain
            .iter()
            .map(|&c| meta.fat_entry_offset(c, fat_index))
            .collect()
    }

    /// Write the constructed chain into all FAT copies.
    pub fn write_chain<IO: RimIO + ?Sized, M: ClusterMeta>(
        io: &mut IO,
        meta: &impl ClusterMeta,
        chain: &[u32],
    ) -> RimIOResult {
        if chain.is_empty() {
            return Ok(());
        }
        let entries = build_entries::<M>(chain);
        for fi in 0..meta.num_fats() {
            let offs = entry_offsets::<M>(meta, chain, fi);
            io.write_multi_at(&offs, M::ENTRY_SIZE, &entries)?;
        }
        Ok(())
    }

    /// Simple contiguity check (useful for “no-FAT-data” contiguous streams).
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
}
