// SPDX-License-Identifier: MIT

//! exFAT directory and stream extension resolver.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::prelude::*;

use crate::core::cursor::ClusterCursor;
pub use crate::core::resolver::*;

use crate::core::FsCursorError;
use crate::{constant::*, meta::*, types::*};

pub struct ExFatResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
    upcase: Option<crate::upcase::UpcaseHandle>,
    dir_streams: Vec<(u32, bool, u64)>,
    status: FsResolverResult<()>,
}

impl<'a, IO: RimRead + ?Sized> ExFatResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        let upcase = crate::upcase::UpcaseHandle::from_io(io, meta);
        let status = upcase.as_ref().map(|_| ()).map_err(|e| *e);
        let upcase = upcase.ok();
        Self {
            io,
            meta,
            upcase,
            dir_streams: Vec::new(),
            status,
        }
    }

    /// Internal helper to get the entry details
    pub fn resolve_entry(&mut self, path: &str) -> FsResolverResult<ExFatEntries> {
        self.status?;
        let entry = crate::core::resolver::walker::walk_path(self, path)?.ok_or(
            FsResolverError::Invalid("Cannot resolve file entry for root"),
        )?;
        Ok(entry)
    }

    pub fn resolve_entry_info(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        self.status?;
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
    type NodeId = u32;

    fn root_node(&self) -> Self::NodeId {
        self.meta.root_unit()
    }

    fn find_entry(
        &mut self,
        dir_cluster: Self::NodeId,
        name: &str,
    ) -> FsResolverResult<Option<Self::Entry>> {
        let (is_contiguous, data_len) = self
            .dir_streams
            .iter()
            .find(|(c, _, _)| *c == dir_cluster)
            .map(|(_, contig, len)| (*contig, *len))
            .unwrap_or((false, 0));
        let res = find_in_dir(
            self.io,
            self.meta,
            dir_cluster,
            is_contiguous,
            data_len,
            name,
            self.upcase.as_ref(),
        )?;
        if let Some(ref e) = res
            && e.is_dir()
        {
            if self.dir_streams.len() >= 256 {
                self.dir_streams.clear();
            }
            self.dir_streams
                .retain(|(cluster, _, _)| *cluster != e.first_cluster());
            self.dir_streams.push((
                e.first_cluster(),
                e.stream.is_contiguous(),
                e.stream.data_length.get(),
            ));
        }
        Ok(res)
    }

    fn is_dir(&self, entry: &Self::Entry) -> bool {
        entry.is_dir()
    }

    fn entry_node(&self, entry: &Self::Entry) -> Self::NodeId {
        entry.first_cluster()
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for ExFatResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_entry_info(path)?;
        crate::ensure!(is_dir, FsResolverError::Invalid("Not a directory"));

        let (is_contiguous, data_len) = self
            .dir_streams
            .iter()
            .find(|(c, _, _)| *c == cluster)
            .map(|(_, contig, len)| (*contig, *len))
            .unwrap_or((false, 0));

        let entries = read_dir_entries(self.io, self.meta, cluster, is_contiguous, data_len)?;
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
        crate::ensure!(!entry.is_dir(), FsResolverError::Invalid("Not a file"));

        let size = entry.stream.data_length.get();
        if entry.stream.valid_data_length.get() > size {
            return Err(FsResolverError::Invalid("Invalid valid data length"));
        }
        if size == 0 {
            return Ok(alloc::boxed::Box::new(rimio::SliceRimIO::new(&[])));
        }

        let first_cluster = entry.first_cluster();
        let is_contiguous = entry.stream.is_contiguous();
        let total_size = entry.stream.data_length.get();
        let valid_size = entry.stream.valid_data_length.get();
        if valid_size > total_size {
            return Err(FsResolverError::Invalid("Invalid valid data length"));
        }

        if first_cluster < 2 || first_cluster > self.meta.last_data_unit() {
            return Err(FsResolverError::Invalid("Invalid first cluster"));
        }
        if is_contiguous {
            let count = total_size.div_ceil(self.meta.unit_size());
            if first_cluster as u64 + count > self.meta.last_data_unit() as u64 + 1 {
                return Err(FsResolverError::Invalid("Contiguous file exceeds volume"));
            }
            let phys_offset = self.meta.unit_offset(first_cluster);
            if valid_size == total_size {
                return Ok(alloc::boxed::Box::new(
                    rimio::extent::ExtentRimRead::from_contiguous(
                        &mut *self.io,
                        phys_offset,
                        total_size,
                    ),
                ));
            } else {
                let extents = vec![
                    rimio::extent::IoExtent {
                        logical_offset: 0,
                        source_offset: Some(phys_offset),
                        len: valid_size,
                    },
                    rimio::extent::IoExtent {
                        logical_offset: valid_size,
                        source_offset: None,
                        len: total_size - valid_size,
                    },
                ];
                return Ok(alloc::boxed::Box::new(rimio::extent::ExtentRimRead::new(
                    &mut *self.io,
                    extents,
                    total_size,
                )));
            }
        }

        let cs = self.meta.unit_size();
        let mut extents = Vec::new();
        let mut logical_offset = 0u64;
        let mut cur = ClusterCursor::new_safe(self.meta, first_cluster);
        cur.for_each_run(self.io, |_io, start, len| {
            if logical_offset >= total_size {
                return Ok(());
            }
            let phys_offset = self.meta.unit_offset(start);
            let run_bytes = (len as u64) * cs;
            let run_len = core::cmp::min(run_bytes, total_size - logical_offset);

            if logical_offset < valid_size {
                let valid_part = core::cmp::min(run_len, valid_size - logical_offset);
                extents.push(rimio::extent::IoExtent {
                    logical_offset,
                    source_offset: Some(phys_offset),
                    len: valid_part,
                });
                if valid_part < run_len {
                    extents.push(rimio::extent::IoExtent {
                        logical_offset: logical_offset + valid_part,
                        source_offset: None,
                        len: run_len - valid_part,
                    });
                }
            } else {
                extents.push(rimio::extent::IoExtent {
                    logical_offset,
                    source_offset: None,
                    len: run_len,
                });
            }
            logical_offset += run_len;
            Ok(())
        })?;

        if logical_offset != total_size {
            return Err(FsResolverError::Invalid("Incomplete file allocation"));
        }

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

fn for_each_dir_run<IO, F>(
    io: &mut IO,
    meta: &ExFatMeta,
    dir_cluster: u32,
    is_contiguous: bool,
    data_len: u64,
    mut f: F,
) -> Result<(), FsCursorError>
where
    IO: RimRead + ?Sized,
    F: FnMut(&mut IO, u32, u32) -> Result<(), FsCursorError>,
{
    if is_contiguous {
        let cs = meta.unit_size();
        let num_clusters = if data_len > 0 {
            data_len.div_ceil(cs) as u32
        } else {
            1
        };
        f(io, dir_cluster, num_clusters)
    } else {
        let mut cur = ClusterCursor::new(meta, dir_cluster);
        cur.for_each_run(io, f)
    }
}

fn name_matches(
    entry: &ExFatEntries,
    target: &str,
    upcase: Option<&crate::upcase::UpcaseHandle>,
) -> bool {
    if let Ok(name) = entry.name() {
        if let Some(u) = upcase {
            let n_chars = name.encode_utf16();
            let t_chars = target.encode_utf16();
            n_chars.map(|c| u.upper(c)).eq(t_chars.map(|c| u.upper(c)))
        } else {
            name.eq_ignore_ascii_case(target)
        }
    } else {
        false
    }
}

fn read_dir_entries<IO: RimRead + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    start_cluster: u32,
    is_contiguous: bool,
    data_len: u64,
) -> FsResolverResult<Vec<ExFatEntries>> {
    const ERR_EOD: &str = "eod";

    let cs = meta.unit_size() as usize;
    let mut entries: Vec<ExFatEntries> = Vec::new();

    let mut lfn_stack: Vec<[u8; 32]> = Vec::with_capacity(16);

    let mut raw_primary: Option<[u8; 32]> = None;
    let mut raw_stream: Option<[u8; 32]> = None;

    let mut buf: Vec<u8> = Vec::new();

    let res = for_each_dir_run(
        io,
        meta,
        start_cluster,
        is_contiguous,
        data_len,
        |io, run_start, run_len| {
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
        },
    );

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
    is_contiguous: bool,
    data_len: u64,
    target: &str,
    upcase: Option<&crate::upcase::UpcaseHandle>,
) -> FsResolverResult<Option<ExFatEntries>> {
    let cs = meta.unit_size() as usize;

    // Assembly state persistent across runs
    let mut lfn_stack = vec![];
    let mut raw_primary: Option<[u8; 32]> = None;
    let mut raw_stream: Option<[u8; 32]> = None;

    // Captured result + sentinel for early-exit
    let mut found: Option<ExFatEntries> = None;

    let mut data: Vec<u8> = Vec::new();
    let res = for_each_dir_run(
        io,
        meta,
        dir_cluster,
        is_contiguous,
        data_len,
        |io, run_start, run_len| {
            let total = (run_len as usize) * cs;
            if data.len() != total {
                data.resize(total, 0u8);
            }
            let off0 = meta.unit_offset(run_start);
            io.read_block_best_effort(off0, &mut data, total)?;

            // Traverse 32-byte entries
            for chunk in data.chunks_exact(32) {
                match chunk[0] {
                    EXFAT_ENTRY_PRIMARY => {
                        // Try to finalize the entry currently being assembled
                        if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take())
                            && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                            && name_matches(&e, target, upcase)
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
                            if name_matches(&e, target, upcase) {
                                found = Some(e);
                                return Err(FsCursorError::Other("found"));
                            }
                        }
                        return Err(FsCursorError::Other("eod")); // Stop the entire chain
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
        },
    );

    match res {
        Err(FsCursorError::Other("eod")) => Ok(None),
        Ok(()) => {
            // End of chain without EOD: final flush in case PRIMARY/STREAM was in progress
            if let (Some(p), Some(s)) = (raw_primary, raw_stream)
                && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                && name_matches(&e, target, upcase)
            {
                return Ok(Some(e));
            }
            Ok(found)
        }
        Err(FsCursorError::Other("found")) => Ok(found),
        Err(e) => Err(FsResolverError::Cursor(e)),
    }
}
