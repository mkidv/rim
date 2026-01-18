// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, vec, vec::Vec};

use crate::FsMeta;
use crate::core::cursor::ClusterCursor;

use crate::fs::exfat::{constant::*, meta::ExFatMeta, types::ExFatEntries};
use rimio::prelude::*;

pub use crate::core::checker::stats::WalkerStats;

use super::{Finding, FsCheckerResult, VerifyReport};

pub struct ExFatWalker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
    pub reachable_bitmap: Vec<u8>,
}

impl<'a, IO: RimIO + ?Sized> ExFatWalker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        let bit_size = meta.cluster_count.div_ceil(8) as usize;
        Self {
            io,
            meta,
            reachable_bitmap: vec![0u8; bit_size],
        }
    }

    pub fn mark_reachable(&mut self, start_cluster: u32, len_bytes: u64) -> FsCheckerResult<()> {
        if len_bytes == 0 {
            return Ok(());
        }
        let mut chain = ClusterCursor::new(self.meta, start_cluster);
        chain.for_each_run(self.io, |_io, start, len| {
            let s_idx = (start.saturating_sub(EXFAT_FIRST_CLUSTER)) as usize;
            let count = len as usize;
            for i in 0..count {
                if s_idx + i < self.reachable_bitmap.len() * 8 {
                    let byte = (s_idx + i) / 8;
                    let bit = (s_idx + i) % 8;
                    if byte < self.reachable_bitmap.len() {
                        self.reachable_bitmap[byte] |= 1 << bit;
                    }
                }
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn walk_tree(
        &mut self,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<()> {
        let root = self.meta.root_unit();
        let mut stack = vec![(root, 0)]; // (cluster, depth)

        // Mark root chain as reachable
        // Note: Root dir has no strict size in ExFAT, it's a chain.
        // We will mark it as we traverse.

        while let Some((dir_cluster, depth)) = stack.pop() {
            stats.dirs_visited += 1;
            stats.max_depth = stats.max_depth.max(depth);

            if depth > 256 {
                rep.push(Finding::warn(
                    "WALK.DEPTH",
                    "Directory depth limit reached (256)",
                ));
                continue;
            }

            // ReadDir entries
            let children = self.scan_directory(dir_cluster, rep, stats)?;

            // Mark this directory's chain itself as reachable
            // (Note: children might have marked parts of it, but we ensure full coverage here)
            let mut cur = ClusterCursor::new(self.meta, dir_cluster);
            cur.for_each_run(self.io, |_io, start, len| {
                let s_idx = start.saturating_sub(EXFAT_FIRST_CLUSTER) as usize;
                for i in 0..len as usize {
                    let idx = s_idx + i;
                    if idx / 8 < self.reachable_bitmap.len() {
                        self.reachable_bitmap[idx / 8] |= 1 << (idx % 8);
                    }
                }
                Ok(())
            })?;

            for child in children {
                if child.is_dir() {
                    // Check if loop
                    let first = child.first_cluster();
                    let idx = (first.saturating_sub(EXFAT_FIRST_CLUSTER)) as usize;
                    if idx / 8 < self.reachable_bitmap.len() {
                        let is_marked = (self.reachable_bitmap[idx / 8] & (1 << (idx % 8))) != 0;
                        if is_marked {
                            rep.push(Finding::err(
                                "WALK.LOOP",
                                format!("Loop or duplicate ref to dir cluster {first}"),
                            ));
                            continue;
                        }
                    }

                    // Mark and push
                    if child.size() > 0 {
                        self.mark_reachable(child.first_cluster(), child.size() as u64)?;
                    }
                    stack.push((first, depth + 1));
                } else {
                    // File
                    if child.size() > 0 {
                        self.mark_reachable(child.first_cluster(), child.size() as u64)?;
                    }
                    stats.files_found += 1;
                }
            }
        }
        Ok(())
    }

    fn scan_directory(
        &mut self,
        dir_cluster: u32,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<Vec<ExFatEntries>> {
        let cs = self.meta.unit_size();
        let mut entries = Vec::new();

        // State for entry reconstruction
        let mut lfn_stack: Vec<[u8; 32]> = Vec::with_capacity(16);
        let mut raw_primary: Option<[u8; 32]> = None;
        let mut raw_stream: Option<[u8; 32]> = None;

        let mut cur = ClusterCursor::new(self.meta, dir_cluster);

        // We iterate blindly over the chain.
        cur.for_each_run(self.io, |io, run_start, run_len| {
            let total = (run_len as usize) * cs;
            let off0 = self.meta.unit_offset(run_start);

            // Read in chunks to avoid huge allocations if runs are large?
            // For now, read full run (usually fine for checker on reasonable systems)
            // But if run is huge (e.g. fragmented 1GB file treated as dir?), limit buffer.
            // Directories are rarely huge.
            let mut buf = vec![0u8; total];
            io.read_block_best_effort(off0, &mut buf, total)?;

            for chunk in buf.chunks_exact(32) {
                stats.entries_scanned += 1;
                let type_byte = chunk[0];

                match type_byte {
                    EXFAT_ENTRY_PRIMARY => {
                        // Flush previous if exists
                        if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take()) {
                            if let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s) {
                                entries.push(e);
                            } else {
                                rep.push(Finding::warn("WALK.PARSE", "Failed to parse entry set"));
                            }
                        }
                        lfn_stack.clear();
                        if let Ok(arr) = chunk.try_into() {
                            raw_primary = Some(arr);
                        }
                        raw_stream = None;
                    }
                    EXFAT_ENTRY_STREAM => {
                        if let Ok(arr) = chunk.try_into() {
                            raw_stream = Some(arr);
                        }
                    }
                    EXFAT_ENTRY_NAME => {
                        if let Ok(arr) = chunk.try_into() {
                            lfn_stack.push(arr);
                        }
                    }
                    EXFAT_EOD => {
                        // End of dir
                        if let (Some(p), Some(s)) = (raw_primary.take(), raw_stream.take())
                            && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
                        {
                            entries.push(e);
                        }
                        return Ok(());
                    }
                    _ => {
                        if type_byte & 0x80 == 0 {
                            // deleted
                            lfn_stack.clear();
                            raw_primary = None;
                            raw_stream = None;
                        }
                    }
                }
            }
            Ok(())
        })?;

        // Flush final
        if let (Some(p), Some(s)) = (raw_primary, raw_stream)
            && let Ok(e) = ExFatEntries::from_raw(&lfn_stack, &p, &s)
        {
            entries.push(e);
        }

        Ok(entries)
    }
}
