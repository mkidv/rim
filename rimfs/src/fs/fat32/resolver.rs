// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::{BlockIO, BlockIOExt};

use crate::core::cursor::ClusterCursor;
pub use crate::core::resolver::*;

use crate::core::FsCursorError;
use crate::core::utils::path_utils::*;
use crate::fs::fat32::{attr::*, constant::*, meta::*, types::*};

pub struct Fat32Resolver<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Fat32Meta,
}

impl<'a, IO: BlockIO + ?Sized> Fat32Resolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Fat32Meta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: BlockIO + ?Sized> FsResolver for Fat32Resolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_path(path)?;
        if !is_dir {
            return Err(FsResolverError::Invalid("Root path is not a dir"));
        }

        let entries = read_dir_entries(self.io, self.meta, cluster)?;
        let entries_string = entries
            .into_iter()
            .map(|entry| entry.name())
            .collect::<Result<Vec<String>, _>>()?;
        Ok(entries_string)
    }

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let (is_dir, first_cluster, size) = self.resolve_path(path)?;
        if is_dir {
            return Err(FsResolverError::Invalid("Not a file"));
        }
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
            return Err("short_stream_read".into());
        }
        Ok(out)
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
}

fn read_dir_entries<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    start_cluster: u32,
) -> FsResolverResult<Vec<Fat32Entries>> {
    let cs = meta.unit_size();
    let mut out = vec![];
    let mut lfn_stack = vec![];

    let mut cur = ClusterCursor::new(meta, start_cluster);
    cur.for_each_run(io, |io, run_start, run_len| {
        let total = (run_len as usize) * cs;
        let mut data = vec![0u8; total];
        let off0 = meta.unit_offset(run_start);
        io.read_block_best_effort(off0, &mut data, total)?;

        for chunk in data.chunks_exact(32) {
            let first = chunk[0];
            if first == FAT_EOD {
                lfn_stack.clear();
                break;
            }
            if first == FAT_ENTRY_DELETED {
                lfn_stack.clear();
                continue;
            }
            let attr = chunk[11];

            if attr == Fat32Attributes::LFN.bits() {
                lfn_stack.push(chunk.try_into().unwrap());
                continue;
            }

            if attr & Fat32Attributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let name11 = &chunk[0..11];

            if attr & Fat32Attributes::DIRECTORY.bits() != 0
                && (name11 == FAT_DOT_NAME || name11 == FAT_DOTDOT_NAME)
            {
                lfn_stack.clear();
                continue;
            }

            let e = Fat32Entries::from_raw(&lfn_stack, chunk)?;
            lfn_stack.clear();
            out.push(e);
        }
        Ok(())
    })?;

    out.sort_by(|a, b| {
        let na = a.name().unwrap_or_default();
        let nb = b.name().unwrap_or_default();
        na.bytes()
            .map(|c| c.to_ascii_lowercase())
            .cmp(nb.bytes().map(|c| c.to_ascii_lowercase()))
    });
    Ok(out)
}

/// Recherche `target` (insensible à la casse si ton Fat32Entries le gère déjà) dans le dir `dir_cluster`.
/// Retourne la première entrée correspondante, sinon None.
pub fn find_in_dir<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    dir_cluster: u32,
    target: &str,
) -> FsResolverResult<Option<Fat32Entries>> {
    let cs = meta.unit_size();

    // Répertoires → autoriser clusters système (root=2)
    let mut cur = ClusterCursor::new(meta, dir_cluster);

    // LFN persistantes entre clusters ET entre runs
    let mut lfn_stack: Vec<[u8; 32]> = Vec::new();
    let mut found: Option<Fat32Entries> = None;

    // On lit un run complet en un seul read, puis on itère par tranches de 32 octets
    let res = cur.for_each_run(io, |io, run_start, run_len| {
        let total = (run_len as usize) * cs;
        let mut data = vec![0u8; total];
        let off0 = meta.unit_offset(run_start);
        io.read_block_best_effort(off0, &mut data, total)?;

        for chunk in data.chunks_exact(32) {
            let first = chunk[0];
            if first == FAT_EOD {
                return Ok(());
            }
            if first == FAT_ENTRY_DELETED {
                lfn_stack.clear();
                continue;
            }

            let attr = chunk[11];

            if attr == Fat32Attributes::LFN.bits() {
                lfn_stack.push(chunk.try_into().unwrap());
                continue;
            }

            if attr & Fat32Attributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let name11 = &chunk[0..11];

            if attr & Fat32Attributes::DIRECTORY.bits() != 0
                && (name11 == FAT_DOT_NAME || name11 == FAT_DOTDOT_NAME)
            {
                lfn_stack.clear();
                continue;
            }

            // Entrée SFN
            let e = Fat32Entries::from_raw(&lfn_stack, chunk)?;
            lfn_stack.clear();

            if e.name_bytes_eq(target) {
                found = Some(e);
                // Early-exit du run (et donc du for_each_run) via une erreur “sentinelle”
                return Err(FsCursorError::Other("found"));
            }
        }
        Ok(())
    });

    match res {
        Ok(()) => Ok(found),
        Err(FsCursorError::Other("found")) => Ok(found),
        Err(e) => Err(FsResolverError::Cursor(e)),
    }
}
