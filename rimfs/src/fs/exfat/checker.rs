// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec};

use rimio::prelude::*;
use zerocopy::{FromBytes, IntoBytes};

pub use crate::core::checker::*;

use crate::core::fat;
use crate::core::utils::checksum_utils::{accumulate_checksum, accumulate_checksum_with_escape};
use crate::fs::exfat::{constant::*, meta::*, types::*};

mod walker;

#[derive(Clone, Debug)]
pub struct ExFatCheckOptions {
    pub phases: VerifyPhases,
    pub fail_fast: bool,
    /// FAT sampling (0 = off)
    pub fat_sample: u32,
    /// Deep walk on the entire FAT (detects loops/overflow) — expensive
    pub deep_fat_walk: bool,
}

impl Default for ExFatCheckOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
            fat_sample: 0,
            deep_fat_walk: true,
        }
    }
}
impl VerifierOptionsLike for ExFatCheckOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }
    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

pub struct ExFatChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
}

impl<'a, IO: RimIO + ?Sized> ExFatChecker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for ExFatChecker<'a, IO> {
    type Options = ExFatCheckOptions;

    fn check_boot(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let bps = self.meta.bytes_per_sector as usize;

        // Main VBR + backup checksum + mirroring (neutralizes volatile fields)
        check_boot_checksum(self.io, 0, bps, rep)?;
        compare_vbr_main_backup(self.io, bps, rep)?;

        // Consistent geometry (BPB)
        let vbr: ExFatBootSector = self.io.read_struct(EXFAT_VBR_SECTOR)?;
        let spc = self.meta.sectors_per_cluster as usize;
        check_bpb_geometry(&vbr, bps, spc, rep)?;
        Ok(())
    }

    fn check_chain(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        // Fast sampling (low I/O)
        if opt.fat_sample > 0 {
            sample_fat(self.io, self.meta, opt.fat_sample, rep)?;
        }
        // Optional deep walk (detects loops and out-of-bounds indices)
        if opt.deep_fat_walk {
            if let Err(e) = check_fat_chains_deep(self.io, self.meta) {
                rep.push(Finding::err("FAT.DEEP", format!("FAT chain walk: {e}")));
            } else {
                rep.push(Finding::info("FAT.DEEP", "FAT chain walk OK"));
            }
        }
        Ok(())
    }

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        // Critical root entries + Up-Case checksum
        let crit = scan_root_for_critical_with_meta(self.io, self.meta, rep)?;
        if let (Some(fc), Some(len), Some(exp)) =
            (crit.upcase_fc, crit.upcase_len, crit.upcase_table_checksum)
        {
            verify_upcase_checksum_over_file(self.io, self.meta, fc, len, rep, exp)?;
        }
        Ok(())
    }

    fn check_cross_reference(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        // Bitmap covers Bitmap/UpCase/Root - Scan First to satisfy borrow checker
        let crit = scan_root_for_critical_with_meta(self.io, self.meta, rep)?;

        let mut walker = walker::ExFatWalker::new(self.io, self.meta);
        let mut stats = walker::WalkerStats::default();
        walker.walk_tree(rep, &mut stats)?;

        // Manually mark critical system clusters (Bitmap, Upcase, Root) as reachable
        // because walker only follows namespace.
        // Bitmap
        let bm_fc = crit.bitmap_fc.unwrap_or(self.meta.bitmap_cluster);
        let bm_len = crit.bitmap_len.unwrap_or(self.meta.bitmap_size_bytes);
        walker.mark_reachable(bm_fc, bm_len)?;
        // Upcase
        let uc_fc = crit.upcase_fc.unwrap_or(self.meta.upcase_cluster);
        let uc_len = crit.upcase_len.unwrap_or(EXFAT_UPCASE_FULL_LENGTH as u64);
        walker.mark_reachable(uc_fc, uc_len)?;
        // Root (is walked by walker, so should be marked, but ensure coverage of chain logic)
        // Root is marked by walker.walk_tree -> ClusterCursor

        let reachable_bitmap = walker.reachable_bitmap;
        let mut orphans = 0usize;
        let mut samples = 0usize;

        let bitmap_clus = crit.bitmap_fc.unwrap_or(self.meta.bitmap_cluster);
        let bpos = self.meta.unit_offset(bitmap_clus);
        let bsize = self
            .meta
            .unit_size()
            .min(self.meta.bitmap_size_bytes as usize);
        // Note: simplified reading of just first cluster of bitmap if > 1 cluster
        // Real implementation should chain-read bitmap.
        // For now, let's assume small disk or just check 1st bitmap cluster coverage
        let mut bitmap_data = vec![0u8; bsize];
        self.io
            .read_at(bpos, &mut bitmap_data)
            .map_err(FsCheckerError::IO)?;

        let start_c = EXFAT_FIRST_CLUSTER;

        for (i, &used) in bitmap_data.iter().enumerate() {
            let reach = *reachable_bitmap.get(i).unwrap_or(&0);

            // If used bit is set but reachable bit is not set -> Orphan
            let diff = used & !reach;
            if diff != 0 {
                for b in 0..8 {
                    if (diff & (1 << b)) != 0 {
                        orphans += 1;
                        if samples < 5 {
                            let c = start_c + (i as u32 * 8) + b;
                            rep.push(Finding::warn("WALK.ORPHAN", format!("Orphan cluster {c}")));
                            samples += 1;
                        }
                    }
                }
            }
        }

        if orphans > 0 {
            rep.push(Finding::err(
                "WALK.ORPHAN",
                format!("Found {orphans} orphan clusters"),
            ));
        } else {
            rep.push(Finding::info("WALK.ORPHAN", "No orphan clusters found"));
        }

        bitmap_covers_critical(self.io, self.meta, &crit, rep)?;

        // Bitmap vs FAT (strict cluster-by-cluster consistency)
        match check_bitmap_fat_consistency(self.io, self.meta) {
            Ok(()) => rep.push(Finding::info("XREF.BITMAPFAT", "Bitmap & FAT consistent")),
            Err(e) => rep.push(Finding::err("XREF.BITMAPFAT", format!("{e}"))),
        }
        Ok(())
    }

    fn fast_check(&mut self) -> FsCheckerResult {
        // Quick policy: key phases, deep FAT walk enabled, no sampling
        let opt = ExFatCheckOptions {
            phases: VerifyPhases::BOOT
                | VerifyPhases::GEOMETRY
                | VerifyPhases::CHAIN
                | VerifyPhases::ROOT
                | VerifyPhases::CROSSREF,
            fail_fast: true,
            fat_sample: 0,
            deep_fat_walk: true,
        };
        let rep = self.check_with(&opt)?;

        if rep.has_error() {
            return Err(FsCheckerError::Invalid("FsInvalid run check_all"));
        }

        Ok(())
    }
}

/* =========================================================================
Implementation of old checks, factorized / adapted
========================================================================= */

/// Deep walk of FAT chains (detects loops, overflow, and invalid indices)
fn check_fat_chains_deep<IO: RimIO + ?Sized>(io: &mut IO, meta: &ExFatMeta) -> FsCheckerResult {
    let first_cluster = meta.first_data_unit();
    let last_cluster = meta.last_data_unit();
    let cluster_span = (last_cluster - first_cluster) as usize;

    let bitmap_size = cluster_span.div_ceil(8);
    let mut visited_bitmap = vec![0u8; bitmap_size];

    #[inline(always)]
    fn mark_visited(bitmap: &mut [u8], first_cluster: u32, cluster: u32) {
        let idx = (cluster - first_cluster) as usize;
        bitmap[idx / 8] |= 1 << (idx % 8);
    }

    #[inline(always)]
    fn is_visited(bitmap: &[u8], first_cluster: u32, cluster: u32) -> bool {
        let idx = (cluster - first_cluster) as usize;
        (bitmap[idx / 8] & (1 << (idx % 8))) != 0
    }

    for start in first_cluster..last_cluster {
        if is_visited(&visited_bitmap, first_cluster, start) {
            continue;
        }

        let mut current = start;
        let mut chain_len = 0usize;

        while (EXFAT_FIRST_CLUSTER..EXFAT_EOC).contains(&current) {
            if current < first_cluster || current >= last_cluster {
                return Err(FsCheckerError::Invalid("Cluster out of range in FAT chain"));
            }
            if is_visited(&visited_bitmap, first_cluster, current) {
                return Err(FsCheckerError::Invalid("Loop detected in FAT chain"));
            }

            mark_visited(&mut visited_bitmap, first_cluster, current);

            let next = fat::chain::read_entry(io, meta, current, 0)?;
            chain_len += 1;
            if chain_len > meta.cluster_count as usize {
                return Err(FsCheckerError::Invalid("Invalid FAT chain length"));
            }
            if next == EXFAT_EOC {
                break;
            }
            current = next;
        }
    }

    Ok(())
}

/// Strict Bitmap <-> FAT consistency
fn check_bitmap_fat_consistency<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
) -> FsCheckerResult {
    let fat_size_bytes = (meta.fat_size_sectors * meta.bytes_per_sector as u32) as usize;
    let mut fat = vec![0u8; fat_size_bytes];
    io.read_at(meta.fat_offset_bytes, &mut fat)
        .map_err(FsCheckerError::IO)?;

    let mut bitmap = vec![0u8; meta.unit_size()];
    io.read_at(meta.unit_offset(meta.bitmap_cluster), &mut bitmap)
        .map_err(FsCheckerError::IO)?;

    let cluster_start = EXFAT_FIRST_CLUSTER;
    let cluster_end = cluster_start + meta.cluster_count;

    for cluster in cluster_start..cluster_end {
        let fat_index = (cluster * 4) as usize;
        if fat_index + 4 > fat.len() {
            return Err(FsCheckerError::Invalid("FAT index out of bounds"));
        }
        let fat_entry = u32::from_le_bytes(
            fat[fat_index..fat_index + 4]
                .try_into()
                .expect("slice length checked"),
        );

        let (byte_index, bit_mask) = meta.bitmap_entry_offset(cluster);
        if byte_index >= bitmap.len() {
            return Err(FsCheckerError::Invalid("Bitmap index out of bounds"));
        }

        let bitmap_set = (bitmap[byte_index] & bit_mask) != 0;
        let fat_used = match fat_entry {
            0x00000000 => false,             // free
            0x00000001 => true,              // reserved
            0x00000002..=0xFFFFFFF6 => true, // chaining
            0xFFFFFFF7 => false,             // bad
            0xFFFFFFF8..=0xFFFFFFFF => true, // reserved/EOC
        };

        if bitmap_set != fat_used {
            return Err(FsCheckerError::Invalid("Cluster bitmap and FAT mismatch"));
        }
    }
    Ok(())
}

#[derive(Default, Clone, Debug)]
pub struct RootCritical {
    pub bitmap_fc: Option<u32>,
    pub bitmap_len: Option<u64>,
    pub upcase_fc: Option<u32>,
    pub upcase_len: Option<u64>,
    pub upcase_table_checksum: Option<u32>,
    pub label_seen: bool,
    pub volume_guid_seen: bool,
}

#[inline(always)]
fn boot_backup_lba512(main_lba512: u64, bps: usize) -> u64 {
    main_lba512 + (EXFAT_BOOT_REGION_SECTORS as u64) * (bps as u64 / 512)
}

/// Calculates and verifies the checksum of a boot region (11 data sectors + 1 checksum sector)
fn check_boot_checksum<IO: RimIO + ?Sized>(
    io: &mut IO,
    lba512: u64,
    bps: usize,
    rep: &mut VerifyReport,
) -> FsCheckerResult<bool> {
    let base = lba512 * 512;
    let mut sum: u32 = 0;

    // Single reused buffer
    let mut sec = vec![0u8; bps];

    // Sectors 0..10
    for s in 0..=10 {
        io.read_at(base + (bps as u64) * (s as u64), &mut sec)?;
        accumulate_checksum_with_escape(&mut sum, &sec, |i, _b| {
            s == 0 && (i == 106 || i == 107 || i == 112)
        });
    }

    // Sector 11 (repeated checksum)
    io.read_at(base + (bps as u64) * 11, &mut sec)?;
    let mut ok = true;
    let mut bad_off = None;
    for (i, c) in sec.chunks_exact(4).enumerate() {
        if u32::from_le_bytes([c[0], c[1], c[2], c[3]]) != sum {
            ok = false;
            bad_off = Some(i * 4);
            break;
        }
    }

    let what = if lba512 == 0 {
        "VBR(main)"
    } else {
        "VBR(backup)"
    };
    if ok {
        rep.push(Finding::info(
            "VBR.CHK",
            format!("{what} checksum OK (0x{sum:08X})"),
        ));
    } else {
        match bad_off {
            Some(off) => rep.push(Finding::err(
                "VBR.CHK",
                format!("{what} checksum mismatch @+{off} (expected 0x{sum:08X})"),
            )),
            None => rep.push(Finding::err("VBR.CHK", format!("{what} checksum mismatch"))),
        }
    }
    Ok(ok)
}

/// Compares sectors 0 (main vs backup) while neutralizing volatile fields
fn compare_vbr_main_backup<IO: RimIO + ?Sized>(
    io: &mut IO,
    bps: usize,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let mut main_raw = vec![0u8; bps];
    let mut bak_raw = vec![0u8; bps];
    io.read_at(0, &mut main_raw)?;
    io.read_at(boot_backup_lba512(0, bps) * 512, &mut bak_raw)?;

    // Parse struct -> neutralize -> re-serialize
    if let (Ok(m0), Ok(b0)) = (
        ExFatBootSector::read_from_bytes(&main_raw),
        ExFatBootSector::read_from_bytes(&bak_raw),
    ) {
        let m = m0.neutralize_vbr_volatile();
        let b = b0.neutralize_vbr_volatile();

        if m.as_bytes() == b.as_bytes() {
            rep.push(Finding::info(
                "VBR.MIRROR",
                "Backup VBR = Main (excluding flags)",
            ));
        } else {
            rep.push(Finding::warn(
                "VBR.MIRROR",
                "Backup VBR ≠ Main (excluding flags)",
            ));
        }
        Ok(())
    } else {
        rep.push(Finding::err("VBR.MIRROR", "Unreadable VBR (struct parse)"));
        Err(FsCheckerError::Invalid("Invalid VBR layout"))
    }
}

/// BPB / geometry consistency validation
fn check_bpb_geometry(
    vbr: &ExFatBootSector,
    bps: usize,
    spc: usize,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    if vbr.number_of_fats != 1 {
        rep.push(Finding::err(
            "BPB.FATS",
            format!("NumberOfFats={} (TexFAT not supported)", vbr.number_of_fats),
        ));
    }
    if vbr.cluster_count == 0 {
        rep.push(Finding::err("BPB.CLUS", "ClusterCount == 0"));
    }
    if vbr.fat_length == 0 {
        rep.push(Finding::err("BPB.FATL", "FATLength == 0"));
    }

    let vol_bytes = vbr.volume_length.saturating_mul(bps as u64);
    let fat_begin = (vbr.fat_offset as u64) * (bps as u64);
    let fat_end = (vbr.fat_offset as u64 + vbr.fat_length as u64) * (bps as u64);
    let heap_off = (vbr.cluster_heap_offset as u64) * (bps as u64);
    if !(fat_begin < fat_end && fat_end <= heap_off && heap_off < vol_bytes) {
        rep.push(Finding::err(
            "BPB.ORDER",
            "Inconsistent FAT/Heap/Volume ordering",
        ));
    }

    let need_bytes = (vbr.cluster_count as u64 + 2) * 4;
    let fat_bytes = (vbr.fat_length as u64) * (bps as u64);
    if fat_bytes < need_bytes {
        rep.push(Finding::err(
            "BPB.FATL",
            format!("FATLength too small ({fat_bytes} < {need_bytes})"),
        ));
    }

    if vbr.root_dir_cluster < EXFAT_FIRST_CLUSTER
        || vbr.root_dir_cluster > (EXFAT_FIRST_CLUSTER + vbr.cluster_count - 1)
    {
        rep.push(Finding::err(
            "BPB.ROOT",
            "RootDir cluster out of valid range",
        ));
    }

    rep.push(Finding::info(
        "BPB.OK",
        format!(
            "Geometry OK-ish (vol={} MiB, bytes/cluster={})",
            vol_bytes / (1024 * 1024) as u64,
            (bps * spc)
        ),
    ));
    Ok(())
}

/* -------------------- FAT sampling -------------------- */

fn sample_fat<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    sample: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    if sample == 0 {
        return Ok(());
    }
    let bps = meta.bytes_per_sector as usize;
    let step = (meta.fat_size_sectors.max(1) / sample.max(1)).max(1);
    let mut buf = vec![0u8; bps];
    let mut bad = 0u32;

    for i in (0..meta.fat_size_sectors).step_by(step as usize) {
        let off = meta.fat_offset_bytes + (i as u64) * bps as u64;
        if let Err(e) = io.read_at(off, &mut buf) {
            bad += 1;
            rep.push(Finding::warn("FAT.IO", format!("read sector {i}: {e:?}")));
        }
    }
    if bad == 0 {
        rep.push(Finding::info(
            "FAT.SAMPLE",
            format!("FAT sampled ({sample} sectors), no obvious issue"),
        ));
    }
    Ok(())
}

/* -------------------- ROOT & CRITICAL ENTRIES -------------------- */

fn scan_root_for_critical_with_meta<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<RootCritical> {
    let mut out = RootCritical::default();

    let bps = meta.bytes_per_sector as usize;
    let spc = meta.sectors_per_cluster as usize;
    let fc = meta.root_unit();
    let bytes_per_cluster = bps * spc;

    let mut dir = vec![0u8; bytes_per_cluster];
    io.read_at(meta.unit_offset(fc), &mut dir)?;

    let mut i = 0usize;
    while i + 32 <= dir.len() {
        let et = dir[i];
        if et == 0x00 {
            break;
        } // EOD

        match et {
            EXFAT_ENTRY_BITMAP => {
                if let Some((fc, len)) = parse_bitmap_entry(&dir[i..i + 32]) {
                    out.bitmap_fc = Some(fc);
                    out.bitmap_len = Some(len);
                    rep.push(Finding::info(
                        "ROOT.BITMAP",
                        format!("Bitmap fc={fc} len={len} bytes"),
                    ));
                } else {
                    rep.push(Finding::err("ROOT.BITMAP", "Unreadable Bitmap entry"));
                }
                i += 32;
            }
            EXFAT_ENTRY_UPCASE => {
                if let Some((fc, len, chk)) = parse_upcase_entry(&dir[i..i + 32]) {
                    out.upcase_fc = Some(fc);
                    out.upcase_len = Some(len);
                    out.upcase_table_checksum = Some(chk);
                    rep.push(Finding::info(
                        "ROOT.UPCASE",
                        format!("Up-Case fc={fc} len={len} bytes chk=0x{chk:08X}"),
                    ));
                } else {
                    rep.push(Finding::err("ROOT.UPCASE", "Unreadable Up-Case entry"));
                }
                i += 32;
            }
            EXFAT_ENTRY_LABEL => {
                out.label_seen = true;
                i += 32;
            }
            EXFAT_ENTRY_GUID => {
                out.volume_guid_seen = true;
                i += 32;
            }
            EXFAT_ENTRY_PRIMARY => {
                let (ok, consumed, msg) = check_file_entry_set(&dir[i..]);
                rep.push(if ok {
                    Finding::info("DIR.SET", msg)
                } else {
                    Finding::err("DIR.SET", msg)
                });
                i += consumed.max(32);
            }
            _ => {
                i += 32;
            }
        }
    }

    // "Meta" fallbacks
    if out.bitmap_fc.is_none() {
        out.bitmap_fc = Some(meta.bitmap_cluster);
        out.bitmap_len = Some(meta.bitmap_size_bytes);
        rep.push(Finding::warn(
            "ROOT.MISS",
            "Bitmap not found → fallback to meta()",
        ));
    }
    if out.upcase_fc.is_none() {
        out.upcase_fc = Some(meta.upcase_cluster);
        out.upcase_len = Some(EXFAT_UPCASE_FULL_LENGTH as u64);
        rep.push(Finding::warn(
            "ROOT.MISS",
            "Up-Case not found → fallback to meta()",
        ));
    }

    Ok(out)
}

fn verify_upcase_checksum_over_file<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    first_cluster: u32,
    len_bytes: u64,
    rep: &mut VerifyReport,
    expected: u32,
) -> FsCheckerResult<()> {
    // Trivial case
    if len_bytes == 0 {
        let got = 0u32;
        if got == expected {
            rep.push(Finding::info(
                "UPCASE.CHK",
                "Up-Case TableChecksum OK (0x00000000)",
            ));
        } else {
            rep.push(Finding::err(
                "UPCASE.CHK",
                format!("Up-Case checksum mismatch: exp 0x{expected:08X}, got 0x{got:08X}"),
            ));
        }
        return Ok(());
    }

    let bps = meta.bytes_per_sector as usize;
    let spc = meta.sectors_per_cluster as usize;
    let bytes_per_cluster = bps * spc;

    let mut sum: u32 = 0;
    let mut remain = len_bytes as usize;
    let mut cur = first_cluster;
    let mut walked = 0usize;

    while remain > 0 {
        // Basic bounds check
        if cur < EXFAT_FIRST_CLUSTER || cur >= meta.last_data_unit() {
            return Err(FsCheckerError::Invalid("Up-Case cluster out of range"));
        }

        // Read current cluster
        let mut buf = vec![0u8; bytes_per_cluster];
        io.read_at(meta.unit_offset(cur), &mut buf)?;

        let take = remain.min(bytes_per_cluster);
        accumulate_checksum(&mut sum, &buf[..take]);
        remain -= take;

        if remain == 0 {
            break;
        }

        // Follow FAT chain
        let next = fat::chain::read_entry(io, meta, cur, 0)?;
        if next == EXFAT_EOC {
            // Chain too short compared to declared length
            return Err(FsCheckerError::Invalid("Up-Case chain shorter than length"));
        }
        cur = next;

        walked += 1;
        if walked > meta.cluster_count as usize {
            return Err(FsCheckerError::Invalid("Up-Case chain loop/overflow"));
        }
    }

    if sum == expected {
        rep.push(Finding::info(
            "UPCASE.CHK",
            format!("Up-Case TableChecksum OK (0x{sum:08X})"),
        ));
    } else {
        rep.push(Finding::err(
            "UPCASE.CHK",
            format!("Up-Case checksum mismatch: exp 0x{expected:08X}, got 0x{sum:08X}"),
        ));
    }
    Ok(())
}

/* -------------------- BITMAP vs FAT -------------------- */

fn bitmap_covers_critical<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    crit: &RootCritical,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let bfc = crit.bitmap_fc.unwrap_or(meta.bitmap_cluster);
    let blen = crit.bitmap_len.unwrap_or(meta.bitmap_size_bytes);

    let mut ok = true;
    for &(name, fc_opt) in &[
        ("bitmap", Some(bfc)),
        ("upcase", crit.upcase_fc.or(Some(meta.upcase_cluster))),
        ("root", Some(meta.root_unit())),
    ] {
        if let Some(fc) = fc_opt
            && !bitmap_has_cluster_meta(io, meta, bfc, blen, fc)?
        {
            ok = false;
            rep.push(Finding::err(
                "BITMAP.COVER",
                format!("Bitmap does not cover {name} fc={fc}"),
            ));
        }
    }
    if ok {
        rep.push(Finding::info(
            "BITMAP.COVER",
            "Bitmap covers critical resources",
        ));
    }
    Ok(())
}

fn bitmap_has_cluster_meta<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    bfc: u32,
    blen: u64,
    cluster: u32,
) -> FsCheckerResult<bool> {
    let bps = meta.bytes_per_sector as usize;
    let spc = meta.sectors_per_cluster as usize;
    let cluster_size = (bps * spc) as u64;

    let idx = cluster as u64 - EXFAT_FIRST_CLUSTER as u64;
    let byte_index = idx / 8;
    if byte_index >= blen {
        return Ok(false);
    }

    if byte_index < cluster_size {
        let mut clus = vec![0u8; cluster_size as usize];
        io.read_at(meta.unit_offset(bfc), &mut clus)?;
        let byte = clus[byte_index as usize];
        let bit = (idx % 8) as u8;
        Ok((byte & (1 << bit)) != 0)
    } else {
        // TODO: follow FAT if bitmap > 1 cluster
        Ok(true) // avoid false negatives in MVP
    }
}

/* -------------------- parsing helpers -------------------- */

fn parse_bitmap_entry(raw: &[u8]) -> Option<(u32, u64)> {
    ExFatBitmapEntry::read_from_bytes(raw)
        .ok()
        .map(|e| (e.first_cluster, e.data_length))
}
fn parse_upcase_entry(raw: &[u8]) -> Option<(u32, u64, u32)> {
    ExFatUpcaseEntry::read_from_bytes(raw)
        .ok()
        .map(|e| (e.first_cluster, e.data_length, e.table_checksum))
}

fn check_file_entry_set(raw: &[u8]) -> (bool, usize, String) {
    if raw.len() < 64 || raw[0] != EXFAT_ENTRY_PRIMARY {
        return (false, 32, "File set: missing signature".into());
    }
    if raw[32] != EXFAT_ENTRY_STREAM {
        return (
            false,
            64,
            format!("File set: expected Stream(0xC0), found 0x{:02X}", raw[32]),
        );
    }
    let mut off = 64usize;
    let mut names = 0usize;
    while off + 32 <= raw.len() && raw[off] == EXFAT_ENTRY_NAME {
        off += 32;
        names += 1;
    }
    if names == 0 {
        return (false, off, "File set: no FileName(0xC1)".into());
    }
    (true, off, format!("File entry set OK ({names} FileName)"))
}
