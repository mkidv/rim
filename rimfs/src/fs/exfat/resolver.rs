// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::{BlockIO, BlockIOExt};

use crate::core::cursor::ClusterCursor;
pub use crate::core::resolver::*;

use crate::core::FsCursorError;
use crate::core::utils::path_utils::*;
use crate::fs::exfat::{constant::*, meta::*, types::*};

pub struct ExFatResolver<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
}

impl<'a, IO: BlockIO + ?Sized> ExFatResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: BlockIO + ?Sized> FsResolver for ExFatResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_path(path)?;
        crate::ensure!(is_dir, FsResolverError::Invalid("Expected a directory"));

        let entries = read_dir_entries(self.io, self.meta, cluster)?;
        let entries_string = entries
            .into_iter()
            .map(|entry| entry.name())
            .collect::<Result<Vec<String>, _>>()?;
        Ok(entries_string)
    }

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let (is_dir, first_cluster, size) = self.resolve_path(path)?;
        crate::ensure!(!is_dir, FsResolverError::Invalid("Expected a file"));

        if size == 0 {
            return Ok(Vec::new());
        }

        let cs = self.meta.unit_size();
        let mut out = vec![0u8; size];
        let mut written = 0usize;

        let mut cur = ClusterCursor::new_safe(self.meta, first_cluster);
        cur.for_each_run(self.io, |io, start, len| {
            if written >= out.len() {
                return Ok(());
            }
            let off = self.meta.unit_offset(start);
            let bytes = (len as usize) * cs;
            let to_copy = core::cmp::min(bytes, out.len() - written);
            io.read_at(off, &mut out[written..written + to_copy])?;
            written += to_copy;
            Ok(())
        })?;

        if written < out.len() {
            crate::bail!("short_stream_read");
        }
        Ok(out)
    }

    fn resolve_path(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        if path.is_empty() || path == "/" {
            return Ok((true, self.meta.root_unit(), 0));
        }
        let components = split_path(path);
        let mut cluster = self.meta.root_unit();

        for (i, comp) in components.iter().enumerate() {
            let entry =
                find_in_dir(self.io, self.meta, cluster, comp)?.ok_or(FsResolverError::NotFound)?;
            let is_last = i == components.len() - 1;
            let is_dir = entry.is_dir();
            let next = entry.first_cluster();
            let size = entry.size();

            if is_last {
                return Ok((is_dir, next, size));
            } else {
                crate::ensure!(is_dir, FsResolverError::Invalid("Expected a directory"));
            }

            cluster = next;
        }
        Err(FsResolverError::Invalid("Invalid path"))
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        if path.is_empty() || path == "/" {
            return Ok(FileAttributes::new_dir());
        }
        let components = split_path(path);
        let mut cluster = self.meta.root_unit();

        for (i, comp) in components.iter().enumerate() {
            let entry =
                find_in_dir(self.io, self.meta, cluster, comp)?.ok_or(FsResolverError::NotFound)?;
            if i == components.len() - 1 {
                return Ok(entry.attr());
            }
            if !entry.is_dir() {
                return Err(FsResolverError::Invalid(
                    "Expected directory for intermediate component",
                ));
            }
            cluster = entry.first_cluster();
        }
        Err(FsResolverError::Invalid("Invalid path"))
    }
}

fn read_dir_entries<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    start_cluster: u32,
) -> FsResolverResult<Vec<ExFatEntries>> {
    const ERR_EOD: &str = "eod";

    let cs = meta.unit_size();
    let mut entries: Vec<ExFatEntries> = Vec::new();

    let mut lfn_stack: Vec<[u8; 32]> = Vec::new();
    lfn_stack.reserve(16);

    let mut raw_primary: Option<[u8; 32]> = None;
    let mut raw_stream: Option<[u8; 32]> = None;

    let mut buf: Vec<u8> = Vec::new();

    let mut cur = ClusterCursor::new(meta, start_cluster);
    let res = cur.for_each_run(io, |io, run_start, run_len| {
        let total = (run_len as usize) * cs;

        if buf.len() != total {
            buf.resize(total, 0u8);
        }
        let off0 = meta.unit_offset(run_start);
        io.read_block_best_effort(off0, &mut buf[..], total)?;

        for chunk in buf[..].chunks_exact(32) {
            match chunk[0] {
                EXFAT_ENTRY_PRIMARY => {
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take()) {
                        if let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s) {
                            entries.push(e);
                        }
                    }
                    lfn_stack.clear();
                    raw_primary = Some(chunk.try_into().unwrap());
                    raw_stream = None;
                }
                EXFAT_ENTRY_STREAM => {
                    raw_stream = Some(chunk.try_into().unwrap());
                }
                EXFAT_ENTRY_NAME => {
                    lfn_stack.push(chunk.try_into().unwrap());
                }
                EXFAT_EOD => {
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take()) {
                        let e = ExFatEntries::from_raw(&lfn_stack, &p, &s)?;
                        entries.push(e);
                    }
                    lfn_stack.clear();
                    return Err(FsCursorError::Other(ERR_EOD));
                }
                _ => {
                    if raw_primary.is_none() && raw_stream.is_none() {
                        continue;
                    }
                    lfn_stack.clear();
                    raw_primary = None;
                    raw_stream = None;
                }
            }
        }
        Ok(())
    });

    match res {
        Ok(()) => {}
        Err(FsCursorError::Other("eod")) => {}
        Err(e) => return Err(FsResolverError::Cursor(e)),
    }

    if let (Some(p), Some(s)) = (raw_primary, raw_stream) {
        if let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s) {
            entries.push(e);
        }
    }

    entries.sort_by(|a, b| {
        let na = a.name().unwrap_or_default();
        let nb = b.name().unwrap_or_default();
        na.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(nb.bytes().map(|c| c.to_ascii_lowercase()))
    });

    Ok(entries)
}

/// Recherche `target` dans le répertoire `dir_cluster` (exFAT).
/// Retourne la première entrée correspondante, sinon None.
/// - Parcours par runs pour limiter les I/O
/// - Autorise les clusters système (root dir)
/// - Garde l'état PRIMARY/STREAM/NAME à cheval entre clusters **et** runs
pub fn find_in_dir<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    dir_cluster: u32,
    target: &str,
) -> FsResolverResult<Option<ExFatEntries>> {
    let cs = meta.unit_size();

    // Répertoires → autoriser système (root, etc.)
    let mut cur = ClusterCursor::new(meta, dir_cluster);

    // État d’assemblage persistant entre runs
    let mut lfn_stack = vec![];
    let mut raw_primary: Option<[u8; 32]> = None;
    let mut raw_stream: Option<[u8; 32]> = None;

    // Résultat capturé + sentinelle pour early-exit
    let mut found: Option<ExFatEntries> = None;

    let res = cur.for_each_run(io, |io, run_start, run_len| {
        // Lire le run d'un bloc
        let total = (run_len as usize) * cs;
        let mut data = vec![0u8; total];
        let off0 = meta.unit_offset(run_start);
        io.read_block_best_effort(off0, &mut data, total)?;

        // Parcourir les entrées 32 octets
        for chunk in data.chunks_exact(32) {
            match chunk[0] {
                EXFAT_ENTRY_PRIMARY => {
                    // Si on avait une entrée en cours, tenter de la finaliser
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take())
                        && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                        && e.name_bytes_eq(target)
                    {
                        found = Some(e);
                        return Err(FsCursorError::Other("found"));
                    }
                    // Démarrer une nouvelle entrée
                    lfn_stack.clear();
                    raw_primary = Some(chunk.try_into().unwrap());
                    raw_stream = None;
                }
                EXFAT_ENTRY_STREAM => {
                    // Associer au PRIMARY courant (s'il existe)
                    raw_stream = Some(chunk.try_into().unwrap());
                }
                EXFAT_ENTRY_NAME => {
                    // Accumuler les NAME (UTF-16LE par fragments)
                    lfn_stack.push(chunk.try_into().unwrap());
                }
                EXFAT_EOD => {
                    // Fin logique du dir: flush la dernière entrée potentielle
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take()) {
                        let e = ExFatEntries::from_raw(&lfn_stack, &p, &s)?;
                        if e.name_bytes_eq(target) {
                            found = Some(e);
                            return Err(FsCursorError::Other("found"));
                        }
                    }
                    return Ok(()); // Plus rien après
                }
                _ => {
                    // Entrée inconnue / padding → si on était en cours, on reset proprement
                    if raw_primary.is_none() && raw_stream.is_none() {
                        continue;
                    }
                    lfn_stack.clear();
                    raw_primary = None;
                    raw_stream = None;
                }
            }
        }
        Ok(())
    });

    match res {
        Ok(()) => {
            // Fin de chaîne sans EOD: flush final au cas où PRIMARY/STREAM était en cours
            if let (Some(p), Some(s)) = (raw_primary, raw_stream)
                && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                && e.name_bytes_eq(target)
            {
                return Ok(Some(e));
            }
            Ok(found)
        }
        Err(FsCursorError::Other("found")) => Ok(found),
        Err(e) => Err(FsResolverError::Cursor(e)),
    }
}
