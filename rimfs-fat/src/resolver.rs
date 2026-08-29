// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::ToString;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::{RimIO, RimIOExt};

use crate::core::cursor::ClusterCursor;
pub use crate::core::resolver::*;

use crate::core::FsCursorError;
use crate::core::fat::{FatDriver, FatFsMeta};
use crate::core::utils::path_utils::*;
use crate::{attr::*, constant::*, meta::*, types::*};

pub struct FatResolver<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a FatMeta,
}

impl<'a, IO: RimIO + ?Sized> FatResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a FatMeta) -> Self {
        Self { io, meta }
    }

    pub fn resolve_entry_info(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        match crate::core::resolver::walker::walk_path(self, path)? {
            Some(entry) => Ok((entry.is_dir(), entry.first_cluster(), entry.size())),
            None => Ok((true, self.meta.root_unit(), 0)),
        }
    }
}

use crate::core::resolver::walker::WalkerDataSource;

impl<'a, IO: RimIO + ?Sized> WalkerDataSource for FatResolver<'a, IO> {
    type Entry = FatEntries;

    fn root_cluster(&self) -> u32 {
        self.meta.root_unit()
    }

    fn find_entry(
        &mut self,
        dir_cluster: u32,
        name: &str,
    ) -> FsResolverResult<Option<Self::Entry>> {
        find_in_dir(self.io, self.meta, dir_cluster, name)
    }

    fn is_dir(&self, entry: &Self::Entry) -> bool {
        entry.is_dir()
    }

    fn entry_cluster(&self, entry: &Self::Entry) -> u32 {
        entry.first_cluster()
    }
}

impl<'a, 'b, IO: RimIO + ?Sized> FsTreeResolver<'b> for FatResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_entry_info(path)?;
        crate::ensure!(is_dir, FsResolverError::Invalid("Root path is not a dir"));

        let entries = read_dir_entries(self.io, self.meta, cluster)?;
        let entries_string = entries
            .into_iter()
            .map(|entry| entry.name())
            .collect::<Result<Vec<String>, _>>()?;
        Ok(entries_string)
    }

    fn open_file(
        &mut self,
        path: &str,
    ) -> FsResolverResult<alloc::boxed::Box<dyn rimio::RimRead + 'b>> {
        let (is_dir, _first_cluster, size) = self.resolve_entry_info(path)?;
        crate::ensure!(!is_dir, FsResolverError::Invalid("Not a file"));
        if size == 0 {
            return Ok(alloc::boxed::Box::new(rimio::SliceRimIO::new(&[])));
        }

        let data = self.read_file(path)?;
        Ok(alloc::boxed::Box::new(rimio::VecRimIO::new(data)))
    }

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let (is_dir, first_cluster, size) = self.resolve_entry_info(path)?;
        crate::ensure!(!is_dir, FsResolverError::Invalid("Not a file"));
        if size == 0 {
            return Ok(Vec::new());
        }

        let cs = self.meta.unit_size();
        let is_contiguous_hint = self.read_attributes(path)?.contiguous;
        let mut is_actually_contiguous = false;

        if is_contiguous_hint && first_cluster >= FAT_FIRST_CLUSTER {
            // Mini-check FAT: verify the first N clusters are indeed contiguous
            // This protects against the hint being stale.
            let mut driver = FatDriver::new(self.meta);
            let check_count = (size.div_ceil(cs)).min(8) as u32;
            let mut current = first_cluster;
            let mut consistent = true;

            for i in 0..check_count {
                let next = driver.get(self.io, current)?;
                let expected = if i + 1 < (size.div_ceil(cs)) as u32 {
                    current + 1
                } else {
                    FatFsMeta::entry_mask(self.meta) // Should be EOC
                };

                if i + 1 < (size.div_ceil(cs)) as u32 {
                    if next != expected {
                        consistent = false;
                        break;
                    }
                } else {
                    // Last check: must be EOC or next cluster in chain
                    if !FatFsMeta::is_eoc(self.meta, next) && next != current + 1 {
                        consistent = false;
                        break;
                    }
                }
                current = next;
            }
            is_actually_contiguous = consistent;
        }

        let mut out = vec![0u8; size];
        let mut written = 0usize;

        if is_actually_contiguous {
            let offset = self.meta.unit_offset(first_cluster);
            self.io.read_at(offset, &mut out)?;
            written = size;
        } else {
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
        }

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
}

fn read_dir_entries<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    start_cluster: u32,
) -> FsResolverResult<Vec<FatEntries>> {
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

            if attr == FatAttributes::LFN.bits() {
                // Safe: chunks_exact(32) guarantees length 32
                if let Ok(arr) = chunk.try_into() {
                    lfn_stack.push(arr);
                }
                continue;
            }

            if attr & FatAttributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let name11 = &chunk[0..11];

            if attr & FatAttributes::DIRECTORY.bits() != 0
                && (name11 == FAT_DOT_NAME || name11 == FAT_DOTDOT_NAME)
            {
                lfn_stack.clear();
                continue;
            }

            let e = FatEntries::from_raw(meta, &lfn_stack, chunk)?;
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

/// Search for `target` (case-insensitive if handled by `FatEntries`) in directory `dir_cluster`.
/// Returns the first matching entry, or `None`.
pub fn find_in_dir<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    dir_cluster: u32,
    target: &str,
) -> FsResolverResult<Option<FatEntries>> {
    let cs = meta.unit_size();

    // Directories -> allow system clusters (root=2)
    let mut cur = ClusterCursor::new(meta, dir_cluster);

    // LFNs persistent across clusters AND across runs
    let mut lfn_stack: Vec<[u8; 32]> = Vec::new();
    let mut found: Option<FatEntries> = None;

    // We read a full run in a single operation, then iterate by 32-byte chunks
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

            if attr == FatAttributes::LFN.bits() {
                // Safe: chunks_exact(32) guarantees length 32
                if let Ok(arr) = chunk.try_into() {
                    lfn_stack.push(arr);
                }
                continue;
            }

            if attr & FatAttributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let name11 = &chunk[0..11];

            if attr & FatAttributes::DIRECTORY.bits() != 0
                && (name11 == FAT_DOT_NAME || name11 == FAT_DOTDOT_NAME)
            {
                lfn_stack.clear();
                continue;
            }

            // SFN entry
            let e = FatEntries::from_raw(meta, &lfn_stack, chunk)?;
            lfn_stack.clear();

            if e.name_bytes_eq(target) {
                found = Some(e);
                // Early-exit from the run (and thus from for_each_run) via a "sentinel" error
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
