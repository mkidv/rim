// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

use crate::{core::cursor::ClusterMeta, fs::exfat::{constant::*, meta::ExFatMeta}};

/// Construit le buffer brut (4 octets par entrée) pour une chaîne FAT.
/// La dernière entrée pointe vers EOC.
#[inline]
pub fn build_fat_chain_entries(chain: &[u32]) -> Vec<u8> {
    let mut out = vec![0u8; chain.len() * EXFAT_ENTRY_SIZE];
    for (i, _) in chain.iter().enumerate() {
        let next = if i + 1 < chain.len() {
            chain[i + 1]
        } else {
            EXFAT_EOC
        };
        let val = next & EXFAT_MASK;
        out[i * 4..i * 4 + 4].copy_from_slice(&val.to_le_bytes());
    }
    out
}

/// Offsets des entrées FAT pour une chaîne donnée (pour une copie donnée).
#[inline(always)]
pub fn fat_entry_offsets_for_chain(meta: &ExFatMeta, chain: &[u32], fat_index: u8) -> Vec<u64> {
    chain
        .iter()
        .map(|&c| meta.fat_entry_offset(c, fat_index))
        .collect()
}

/// Écrit la chaîne dans **toutes** les copies de FAT.
/// Renvoie `rimio::Error` (laissera `?` convertir vers l’erreur FS appelante).
pub fn write_fat_chain<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    chain: &[u32],
) -> BlockIOResult {
    if chain.is_empty() {
        return Ok(());
    }

    let entries = build_fat_chain_entries(chain);

    for fi in 0..meta.num_fats {
        let offsets = fat_entry_offsets_for_chain(meta, chain, fi);
        io.write_multi_at(&offsets, EXFAT_ENTRY_SIZE, &entries)?;
    }

    Ok(())
}
