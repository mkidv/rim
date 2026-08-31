// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::prelude::*;

use crate::core::cursor::ClusterCursor;
pub use crate::core::resolver::*;

use crate::core::FsCursorError;
use crate::core::utils::path_utils::*;
use crate::{constant::*, meta::*, types::*};

pub struct ExFatResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
}

impl<'a, IO: RimRead + ?Sized> ExFatResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        Self { io, meta }
    }

    /// Internal helper to get the entry details
    pub fn resolve_entry(&mut self, path: &str) -> FsResolverResult<ExFatEntries> {
        let entry = crate::core::resolver::walker::walk_path(self, path)?.ok_or(
            FsResolverError::Invalid("Cannot resolve file entry for root"),
        )?;
        Ok(entry)
    }

    pub fn resolve_entry_info(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        if path.is_empty() || path == "/" {
            return Ok((true, self.meta.root_unit(), 0));
        }

        let entry = self.resolve_entry(path)?;
        Ok((entry.is_dir(), entry.first_cluster(), entry.size()))
    }
}

use crate::core::resolver::walker::WalkerDataSource;

impl<'a, IO: RimRead + ?Sized> WalkerDataSource for ExFatResolver<'a, IO> {
    type Entry = ExFatEntries;

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

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for ExFatResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_entry_info(path)?;
        crate::ensure!(is_dir, FsResolverError::Invalid("Expected a directory"));

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
        let entry = self.resolve_entry(path)?;
        crate::ensure!(!entry.is_dir(), FsResolverError::Invalid("Expected a file"));

        let size = entry.size();
        if size == 0 {
            return Ok(alloc::boxed::Box::new(rimio::SliceRimIO::new(&[])));
        }

        let first_cluster = entry.first_cluster();
        let is_contiguous = entry.stream.is_contiguous();
        let total_size = size as u64;

        if is_contiguous {
            let phys_offset = self.meta.unit_offset(first_cluster);
            return Ok(alloc::boxed::Box::new(
                rimio::extent::ExtentRimRead::from_contiguous(
                    &mut *self.io,
                    phys_offset,
                    total_size,
                ),
            ));
        }

        let cs = self.meta.unit_size() as u64;
        let mut extents = Vec::new();
        let mut logical_offset = 0u64;
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

    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let entry = self.resolve_entry(path)?;
        crate::ensure!(!entry.is_dir(), FsResolverError::Invalid("Expected a file"));

        let size = entry.size();
        if size == 0 {
            return Ok(Vec::new());
        }

        let first_cluster = entry.first_cluster();
        let is_contiguous = entry.stream.is_contiguous();

        let mut out = vec![0u8; size];

        if is_contiguous {
            // OPTIMIZATION: Use read_block_best_effort for contiguous file
            let offset = self.meta.unit_offset(first_cluster);
            self.io
                .read_block_best_effort(offset, &mut out, self.meta.unit_size())?;
        } else {
            // Standard path: Use ClusterCursor to chase FAT chain
            let cs = self.meta.unit_size();
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

fn read_dir_entries<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    start_cluster: u32,
) -> FsResolverResult<Vec<ExFatEntries>> {
    const ERR_EOD: &str = "eod";

    let cs = meta.unit_size();
    let mut entries: Vec<ExFatEntries> = Vec::new();

    let mut lfn_stack: Vec<[u8; 32]> = Vec::with_capacity(16);

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
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take())
                        && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                    {
                        entries.push(e);
                    }
                    lfn_stack.clear();
                    raw_primary = Some(chunk.try_into().unwrap_or([0u8; 32]));
                    raw_stream = None;
                }
                EXFAT_ENTRY_STREAM => {
                    raw_stream = Some(chunk.try_into().unwrap_or([0u8; 32]));
                }
                EXFAT_ENTRY_NAME => {
                    lfn_stack.push(chunk.try_into().unwrap_or([0u8; 32]));
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

    if let (Some(p), Some(s)) = (raw_primary, raw_stream)
        && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
    {
        entries.push(e);
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

/// Search for `target` in directory `dir_cluster` (exFAT).
/// Returns the first matching entry, or `None`.
/// - Traversal by runs to minimize I/O.
/// - Allows system clusters (root directory, etc.).
/// - Maintains PRIMARY/STREAM/NAME state across clusters and runs.
pub fn find_in_dir<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    dir_cluster: u32,
    target: &str,
) -> FsResolverResult<Option<ExFatEntries>> {
    let cs = meta.unit_size();

    // Directories -> allow system clusters (root, etc.)
    let mut cur = ClusterCursor::new(meta, dir_cluster);

    // Assembly state persistent across runs
    let mut lfn_stack = vec![];
    let mut raw_primary: Option<[u8; 32]> = None;
    let mut raw_stream: Option<[u8; 32]> = None;

    // Captured result + sentinel for early-exit
    let mut found: Option<ExFatEntries> = None;

    let res = cur.for_each_run(io, |io, run_start, run_len| {
        // Read the run as a single block
        let total = (run_len as usize) * cs;
        let mut data = vec![0u8; total];
        let off0 = meta.unit_offset(run_start);
        io.read_block_best_effort(off0, &mut data, total)?;

        // Traverse 32-byte entries
        for chunk in data.chunks_exact(32) {
            match chunk[0] {
                EXFAT_ENTRY_PRIMARY => {
                    // Try to finalize the entry currently being assembled
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take())
                        && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                        && e.name_bytes_eq(target)
                    {
                        found = Some(e);
                        return Err(FsCursorError::Other("found"));
                    }
                    // Start assembling a new entry
                    lfn_stack.clear();
                    raw_primary = Some(chunk.try_into().unwrap_or([0u8; 32]));
                    raw_stream = None;
                }
                EXFAT_ENTRY_STREAM => {
                    // Associate with the current PRIMARY (if it exists)
                    raw_stream = Some(chunk.try_into().unwrap_or([0u8; 32]));
                }
                EXFAT_ENTRY_NAME => {
                    // Accumulate fragments of the NAME entry (UTF-16LE)
                    lfn_stack.push(chunk.try_into().unwrap_or([0u8; 32]));
                }
                EXFAT_EOD => {
                    // Logical end of directory: flush the last potential entry
                    if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take()) {
                        let e = ExFatEntries::from_raw(&lfn_stack, &p, &s)?;
                        if e.name_bytes_eq(target) {
                            found = Some(e);
                            return Err(FsCursorError::Other("found"));
                        }
                    }
                    return Ok(()); // Nothing follows
                }
                _ => {
                    // Unknown entry or padding -> if an entry was being assembled, reset it
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
            // End of chain without EOD: final flush in case PRIMARY/STREAM was in progress
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
