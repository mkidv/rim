// SPDX-License-Identifier: MIT

//! FAT directory tree and cluster chain resolver.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::{RimRead, RimReadExt};

use crate::core::cursor::ClusterCursor;
pub use crate::core::resolver::*;

use crate::core::FsCursorError;
use crate::{attr::*, constant::*, meta::*, types::*};

pub struct FatResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    meta: &'a FatMeta,
}

impl<'a, IO: RimRead + ?Sized> FatResolver<'a, IO> {
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

impl<'a, IO: RimRead + ?Sized> WalkerDataSource for FatResolver<'a, IO> {
    type Entry = FatEntries;
    type NodeId = u32;

    fn root_node(&self) -> Self::NodeId {
        self.meta.root_unit()
    }

    fn find_entry(
        &mut self,
        dir_cluster: Self::NodeId,
        name: &str,
    ) -> FsResolverResult<Option<Self::Entry>> {
        find_in_dir(self.io, self.meta, dir_cluster, name)
    }

    fn is_dir(&self, entry: &Self::Entry) -> bool {
        entry.is_dir()
    }

    fn entry_node(&self, entry: &Self::Entry) -> Self::NodeId {
        entry.first_cluster()
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for FatResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_entry_info(path)?;
        crate::ensure!(is_dir, FsResolverError::Invalid("Not a directory"));

        let entries = read_dir_entries(self.io, self.meta, cluster)?;
        let entries_string = entries
            .into_iter()
            .map(|entry| entry.name())
            .collect::<Result<Vec<String>, _>>()?;
        Ok(entries_string)
    }

    fn open_file<'c>(
        &'c mut self,
        path: &str,
    ) -> FsResolverResult<alloc::boxed::Box<dyn rimio::RimRead + 'c>> {
        let (is_dir, first_cluster, size) = self.resolve_entry_info(path)?;
        crate::ensure!(!is_dir, FsResolverError::Invalid("Not a file"));
        if size == 0 {
            return Ok(alloc::boxed::Box::new(rimio::SliceRimIO::new(&[])));
        }

        let cs = self.meta.unit_size();
        let mut extents = Vec::new();
        let mut logical_offset = 0u64;
        let total_size = size as u64;

        let mut cur = ClusterCursor::new_safe(self.meta, first_cluster);
        cur.for_each_run(self.io, |_io, start, len| {
            if logical_offset >= total_size {
                return Ok(());
            }
            let phys_offset = self.meta.unit_offset(start);
            let run_bytes = (len as u64) * cs;
            let extent_len = core::cmp::min(run_bytes, total_size - logical_offset);
            extents.push(rimio::extent::IoExtent {
                logical_offset,
                source_offset: Some(phys_offset),
                len: extent_len,
            });
            logical_offset += extent_len;
            Ok(())
        })?;

        Ok(alloc::boxed::Box::new(rimio::extent::ExtentRimRead::new(
            &mut *self.io,
            extents,
            total_size,
        )))
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        match crate::core::resolver::walker::walk_path(self, path)? {
            Some(entry) => Ok(entry.attr()),
            None => Ok(FileAttributes::new_dir()),
        }
    }
}

fn read_dir_entries<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    start_cluster: u32,
) -> FsResolverResult<Vec<FatEntries>> {
    let cs = meta.unit_size() as usize;
    let mut out = vec![];
    let mut lfn_stack = vec![];

    let mut data: Vec<u8> = Vec::new();
    let mut cur = ClusterCursor::new(meta, start_cluster);
    cur.for_each_run(io, |io, run_start, run_len| {
        let total = (run_len as usize) * cs;
        if data.len() != total {
            data.resize(total, 0);
        }
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

            if attr == FatFileAttributes::LFN.bits() {
                // Safe: chunks_exact(32) guarantees length 32
                if let Ok(arr) = chunk.try_into() {
                    lfn_stack.push(arr);
                }
                continue;
            }

            if attr & FatFileAttributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let name11 = &chunk[0..11];

            if attr & FatFileAttributes::DIRECTORY.bits() != 0
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
pub fn find_in_dir<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    dir_cluster: u32,
    target: &str,
) -> FsResolverResult<Option<FatEntries>> {
    let cs = meta.unit_size() as usize;

    // Directories -> allow system clusters (root=2)
    let mut cur = ClusterCursor::new(meta, dir_cluster);

    // LFNs persistent across clusters AND across runs
    let mut lfn_stack: Vec<[u8; 32]> = Vec::new();
    let mut found: Option<FatEntries> = None;

    // We read a full run in a single operation, reusing the buffer across runs
    let mut data: Vec<u8> = Vec::new();
    let res = cur.for_each_run(io, |io, run_start, run_len| {
        let total = (run_len as usize) * cs;
        if data.len() != total {
            data.resize(total, 0);
        }
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

            if attr == FatFileAttributes::LFN.bits() {
                // Safe: chunks_exact(32) guarantees length 32
                if let Ok(arr) = chunk.try_into() {
                    lfn_stack.push(arr);
                }
                continue;
            }

            if attr & FatFileAttributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let name11 = &chunk[0..11];

            if attr & FatFileAttributes::DIRECTORY.bits() != 0
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
