// SPDX-License-Identifier: MIT

//! FAT directory tree reachability walker.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, string::String, vec, vec::Vec};

use crate::FsMeta;
use crate::core::checker::ReachabilityTracker;
pub use crate::core::checker::stats::WalkerStats;
use crate::core::fat::*;
use crate::core::utils::checksum_utils::checksum;
use crate::core::{cursor::ClusterCursor, errors::*};
use crate::{attr::FatFileAttributes, constant::*, meta::FatMeta};
use rimio::prelude::*;

use super::{Finding, FsCheckerResult, VerifyReport};
use crate::types::{FatEntries, FatEntry};
use zerocopy::FromBytes;

pub struct FatWalker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a FatMeta,
    /// Memory-efficient reachability tracker (1 bit per cluster instead of 1 byte)
    pub tracker: ReachabilityTracker,
}

impl<'a, IO: RimIO + ?Sized> FatWalker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a FatMeta) -> Self {
        // Track clusters from FAT_FIRST_CLUSTER (2) up to cluster_count
        let count = meta.cluster_count as usize;
        Self {
            io,
            meta,
            tracker: ReachabilityTracker::new(FAT_FIRST_CLUSTER, count),
        }
    }

    /// Iterates over **all** entries in a directory, reading by runs.
    pub fn scan_directory<F>(
        io: &mut IO,
        meta: &FatMeta,
        start_cluster: u32,
        mut on_entry: F,
    ) -> FsCheckerResult<()>
    where
        F: FnMut(&[[u8; 32]], &[u8]) -> FsCursorResult<()>,
    {
        let mut lfn_stack: Vec<[u8; 32]> = Vec::new();

        // Helper to process raw directory data bytes
        let mut process_data = |data: &[u8]| -> FsCheckerResult<()> {
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

                // LFN piece
                if attr == FatFileAttributes::LFN.bits() {
                    // Safe: chunks_exact(32) guarantees length 32
                    if let Ok(arr) = chunk.try_into() {
                        lfn_stack.push(arr);
                    }
                    continue;
                }

                // Volume label
                if attr & FatFileAttributes::VOLUME_ID.bits() != 0 {
                    lfn_stack.clear();
                    continue;
                }

                // "." / ".."
                let name11 = &chunk[0..11];
                if (attr & FatFileAttributes::DIRECTORY.bits() != 0)
                    && (name11 == FAT_DOT_NAME || name11 == FAT_DOTDOT_NAME)
                {
                    lfn_stack.clear();
                    continue;
                }

                on_entry(&lfn_stack, chunk)?;
                lfn_stack.clear();
            }
            Ok(())
        };

        // Handle Fixed Root Directory (FAT12/16)
        if meta.root_entry_count > 0 && start_cluster == 1 {
            let root_bytes = (meta.root_entry_count as usize) * 32;
            let offset = meta.unit_offset(start_cluster);
            let mut data = vec![0u8; root_bytes];
            io.read_block_best_effort(offset, &mut data, root_bytes)
                .map_err(FsCheckerError::IO)?;

            return process_data(&data);
        }

        // Handle standard Cluster Chain Directory (FAT32 or subdirs)
        let cs = meta.unit_size() as usize;
        let mut cur = ClusterCursor::new(meta, start_cluster);

        cur.for_each_run(io, |io, run_start, run_len| {
            let total = (run_len as usize) * cs;
            let off0 = meta.unit_offset(run_start);
            let mut data = vec![0u8; total];
            io.read_block_best_effort(off0, &mut data, total)?;
            process_data(&data).map_err(|_e| FsCursorError::Other("Checker error in scan"))?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn walk_from_root(
        &mut self,
        check_lfn: bool,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<()> {
        let root = self.meta.root_unit();
        let mut stack = vec![(root, 0)];

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

            let is_fixed_root = self.meta.root_entry_count > 0 && dir_cluster == 1;
            if !is_fixed_root {
                let meta = self.meta;
                let tracker = &mut self.tracker;
                let mut cur = ClusterCursor::new(meta, dir_cluster);
                match cur.for_each_run(self.io, |_io, start, len| {
                    tracker.mark_range(start, len);
                    Ok(())
                }) {
                    Ok(_) => {}
                    Err(e) => {
                        rep.push(Finding::err(
                            "WALK.ERR",
                            format!("Broken dir chain @{dir_cluster}: {e:?}"),
                        ));
                        continue;
                    }
                }
            }

            let mut child_dirs: Vec<u32> = Vec::new();
            let mut file_heads: Vec<u32> = Vec::new();

            if let Err(e) =
                Self::scan_directory(self.io, self.meta, dir_cluster, |lfn_stack, sfn| {
                    stats.entries_scanned += 1;
                    if check_lfn
                        && !lfn_stack.is_empty()
                        && let Err(msg) = validate_lfn_set(lfn_stack, sfn)
                    {
                        rep.push(Finding::warn("LFN.SET", msg));
                    }

                    if self.meta.use_integrity
                        && let Err(e) = FatEntries::from_raw(self.meta, lfn_stack, sfn)
                    {
                        rep.push(Finding::warn(
                            "INT.ENTRY",
                            format!("Integrity error: {e:?}"),
                        ));
                    }

                    let entry = FatEntry::ref_from_bytes(sfn)
                        .map_err(|_| rimio::RimIOError::Invalid("Truncated FAT directory entry"))?;
                    let attr = entry.attr;
                    let first_cluster = entry.first_cluster();

                    if first_cluster >= FAT_FIRST_CLUSTER {
                        if (attr & 0x10) != 0 {
                            if first_cluster != dir_cluster {
                                // Loop detection: check if already reachable (marked)
                                if !self.tracker.is_marked(first_cluster) {
                                    child_dirs.push(first_cluster);
                                } else {
                                    rep.push(Finding::warn(
                                        "WALK.LOOP",
                                        format!(
                                            "Loop or duplicate ref to dir cluster {first_cluster}"
                                        ),
                                    ));
                                }
                            }
                        } else {
                            file_heads.push(first_cluster);
                        }
                    }
                    Ok(())
                })
            {
                rep.push(Finding::err(
                    "WALK.READ",
                    format!("Failed to read dir @{dir_cluster}: {e:?}"),
                ));
            }

            for fc in file_heads {
                let meta = self.meta;
                let tracker = &mut self.tracker;
                let mut cur = ClusterCursor::new(meta, fc);
                if let Err(e) = cur.for_each_run(self.io, |_io, start, len| {
                    tracker.mark_range(start, len);
                    Ok(())
                }) {
                    rep.push(Finding::warn(
                        "WALK.FILE",
                        format!("Broken file chain @{fc}: {e:?}"),
                    ));
                }
            }

            for child in child_dirs {
                stack.push((child, depth + 1));
            }
        }

        Ok(())
    }

    pub fn report_orphans(
        &mut self,
        rep: &mut VerifyReport,
        sample_limit: usize,
    ) -> FsCheckerResult<()> {
        let start = FAT_FIRST_CLUSTER;
        let end = FAT_FIRST_CLUSTER + self.meta.cluster_count - 1;

        let mut samples = 0usize;
        let mut orphans = 0usize;

        // Linear scan of FAT to find used but not reachable
        let mut driver = FatDriver::new(self.meta);
        for c in start..=end {
            let e = driver.read_entry(self.io, c)?;
            let used = e != 0 && e != FAT_BAD_CLUSTER; // 0=Free

            let reach = self.tracker.is_marked(c);
            if used && !reach {
                orphans += 1;
                if samples < sample_limit {
                    rep.push(Finding::warn("FAT.ORPHAN", format!("Orphan cluster {c}")));
                    samples += 1;
                }
            }
        }

        if orphans == 0 {
            rep.push(Finding::info("FAT.ORPHAN", "No orphan clusters"));
        } else {
            rep.push(Finding::warn(
                "FAT.ORPHAN",
                format!("{orphans} orphan clusters (sampled {samples})"),
            ));
        }
        Ok(())
    }
}

fn validate_lfn_set(lfns: &[[u8; 32]], sfn: &[u8]) -> Result<(), String> {
    if lfns.is_empty() {
        return Ok(());
    }

    let first = &lfns[0];
    let n = first[0] & 0x1F;
    let last_bit = (first[0] & 0x40) != 0;

    if !last_bit {
        return Err("LFN last-bit (0x40) missing on last piece".into());
    }
    if n == 0 {
        return Err("LFN ordinal zero".into());
    }
    if n as usize != lfns.len() {
        return Err(format!(
            "LFN count mismatch (ord n={} but {} entries)",
            n,
            lfns.len()
        ));
    }

    for (i, raw) in lfns.iter().enumerate() {
        let ord = raw[0] & 0x1F;
        let expect = n - i as u8;
        if ord != expect {
            return Err(format!("LFN order mismatch (got {ord}, expect {expect})"));
        }
        if i > 0 && (raw[0] & 0x40) != 0 {
            return Err("LFN last-bit set on non-last piece".into());
        }
    }

    let chk: u8 = checksum(&sfn[0..11]);
    for raw in lfns.iter() {
        if raw[13] != chk {
            return Err(format!(
                "LFN checksum mismatch (got 0x{:02X}, expect 0x{:02X})",
                raw[13], chk
            ));
        }
    }
    Ok(())
}
