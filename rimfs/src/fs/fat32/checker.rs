// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;
use zerocopy::FromBytes;

pub use crate::core::checker::*;

use crate::{
    core::{
        cursor::{ClusterCursor, ClusterMeta, read_fat_entry},
        errors::FsCursorError,
    },
    fs::fat32::{attr::Fat32Attributes, constant::*, meta::*, resolver::*, types::*, utils::*},
};
#[derive(Clone, Debug)]
pub struct Fat32CheckOptions {
    pub phases: VerifyPhases,
    pub fail_fast: bool,
    /// Échantillonne N **entrées** FAT (0 = off)
    pub fat_sample: u32,
    /// Marche profonde des chaînes (détecte boucles/hors-bornes)
    pub deep_fat_walk: bool,
    /// Compare les 2 copies de FAT (échantillonné)
    pub compare_fat_copies: bool,
    /// Vérifie FSINFO.free_count / next_free vs mesure réelle (coûteux)
    pub check_fsinfo_consistency: bool,
    /// Valide les LFN (ordre + checksum) dans les répertoires
    pub check_lfn_sets: bool,
    /// Parcours de l’arbre complet (pour orphelins, LFN, etc.)
    pub walk_reachability: bool,
    /// Limites pour éviter les pathologies
    pub max_dirs: usize,
    pub max_entries_per_dir: usize,
    pub orphan_sample_limit: usize,
    /// Tolérance de mismatch FSINFO.free_count (% sur cluster_count)
    pub fsinfo_tolerance_percent: u8,
}

impl Default for Fat32CheckOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
            fat_sample: 0,
            deep_fat_walk: true,
            compare_fat_copies: true,
            check_fsinfo_consistency: false, // lourd → off par défaut
            check_lfn_sets: true,
            walk_reachability: true,
            max_dirs: 100_000,
            max_entries_per_dir: 65_536,
            orphan_sample_limit: 8,
            fsinfo_tolerance_percent: 2,
        }
    }
}

impl VerifierOptionsLike for Fat32CheckOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }
    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

pub struct Fat32Checker<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Fat32Meta,
}

impl<'a, IO: BlockIO + ?Sized> Fat32Checker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Fat32Meta) -> Self {
        Self { io, meta }
    }
}

/* ========================= FsChecker impl ========================= */

impl<'a, IO: BlockIO + ?Sized> FsChecker for Fat32Checker<'a, IO> {
    type Options = Fat32CheckOptions;

    fn check_boot(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let vbr_offset = FAT_VBR_SECTOR * self.meta.bytes_per_sector as u64;

        let vbr: Fat32Vbr = self.io.read_struct(vbr_offset)?;

        // Signatures/labels
        if vbr.signature != FAT_SIGNATURE {
            rep.push(Finding::err("VBR.SIG", "VBR: Missing 0x55AA"));
        } else {
            rep.push(Finding::info("VBR.SIG", "Boot sector signature OK"));
        }
        if &vbr.fs_type != FAT_FS_TYPE {
            rep.push(Finding::err("VBR.FST", "VBR: Invalid FAT32 FS type label"));
        }

        // FSINFO signatures
        let fsinfo_offset = FAT_FSINFO_SECTOR * self.meta.bytes_per_sector as u64;
        let fsinfo: Fat32FsInfo = self.io.read_struct(fsinfo_offset)?;
        if fsinfo.lead_signature != FAT_FSINFO_LEAD_SIGNATURE {
            rep.push(Finding::err("FSI.LEAD", "FSINFO: Missing RRaA"));
        }
        if fsinfo.struct_signature != FAT_FSINFO_STRUCT_SIGNATURE {
            rep.push(Finding::err("FSI.STRU", "FSINFO: Missing rrAa"));
        }
        if fsinfo.trail_signature != FAT_FSINFO_TRAIL_SIGNATURE {
            rep.push(Finding::err("FSI.TAIL", "FSINFO: Missing 0x55AA"));
        }

        compare_vbr_main_backup(self.io, self.meta, rep)?;

        // Géométrie “sanity”
        let bps = self.meta.bytes_per_sector as usize;
        let spc = self.meta.sectors_per_cluster as usize;
        if bps == 0 || (bps & (bps - 1)) != 0 {
            rep.push(Finding::err("BPB.BPS", "BytesPerSector not power of two"));
        }
        if spc == 0 || (spc & (spc - 1)) != 0 {
            rep.push(Finding::err(
                "BPB.SPC",
                "SectorsPerCluster not power of two",
            ));
        }
        if self.meta.num_fats == 0 {
            rep.push(Finding::err("BPB.FATS", "NumberOfFATs == 0"));
        }
        if self.meta.fat_size_sectors == 0 {
            rep.push(Finding::err("BPB.FATL", "FATLength == 0"));
        }
        if self.meta.root_unit() < 2 {
            rep.push(Finding::err("BPB.ROOT", "Root cluster < 2"));
        }
        rep.push(Finding::info(
            "BPB.OK",
            format!(
                "Geometry OK-ish (bps={}, spc={}, fats={}, fat_sectors={})",
                bps, spc, self.meta.num_fats, self.meta.fat_size_sectors
            ),
        ));

        Ok(())
    }

    fn check_chain(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        if opt.fat_sample > 0 {
            sample_fat(self.io, self.meta, opt.fat_sample, rep)?;
        }
        if opt.compare_fat_copies {
            compare_fat_copies_sampled(self.io, self.meta, opt.fat_sample.max(256), rep)?;
        }
        if opt.deep_fat_walk {
            if let Err(e) = check_fat_chains_deep(self.io, self.meta) {
                rep.push(Finding::err("FAT.DEEP", format!("FAT chain walk: {e}")));
            } else {
                rep.push(Finding::info("FAT.DEEP", "FAT chain walk OK"));
            }
        }
        if opt.check_fsinfo_consistency {
            check_fsinfo_consistency(self.io, self.meta, opt.fsinfo_tolerance_percent, rep)?;
        }
        Ok(())
    }

    fn check_root(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        // Lecture root cluster (sanity)
        let bps = self.meta.bytes_per_sector as usize;
        let spc = self.meta.sectors_per_cluster as usize;
        let mut buf = vec![0u8; bps * spc];
        self.io
            .read_at(self.meta.unit_offset(self.meta.root_unit()), &mut buf)
            .map_err(FsCheckerError::IO)?;
        rep.push(Finding::info("ROOT.IO", "Root cluster readable"));

        // FAT[root] ≠ FREE
        let r = self.meta.root_unit();
        let next = read_fat_entry(self.io, self.meta, r, 0)?;
        if next == 0 {
            rep.push(Finding::err("ROOT.FAT", "FAT[root] == FREE (0)"));
        }

        // Option: validation LFN sur tout l’arbre
        if opt.walk_reachability {
            let mut reachable = vec![false; self.meta.cluster_count as usize + 2]; // indexé par cluster id
            let mut stats = WalkStats::default();
            walk_tree_and_validate(self.io, self.meta, opt, rep, &mut reachable, &mut stats)?;
            rep.push(Finding::info(
                "DIR.WALK",
                format!(
                    "Walked {} dirs, {} entries (LFN checked: {})",
                    stats.dirs, stats.entries, opt.check_lfn_sets
                ),
            ));

            // Orphelins: FAT “used” mais inatteignables depuis / (hors cluster<2)
            report_orphans(self.io, self.meta, rep, &reachable, opt.orphan_sample_limit)?;
        }

        Ok(())
    }

    fn check_cross_reference(
        &mut self,
        _opt: &Self::Options,
        _rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        // Rien de “bitmap” côté FAT32. (Tu peux ajouter ici d’autres recoupements si besoin.)
        Ok(())
    }

    fn fast_check(&mut self) -> FsCheckerResult {
        let opt = Fat32CheckOptions {
            phases: VerifyPhases::BOOT | VerifyPhases::CHAIN | VerifyPhases::ROOT,
            fail_fast: true,
            fat_sample: 0,
            deep_fat_walk: true,
            compare_fat_copies: true,
            check_fsinfo_consistency: false,
            check_lfn_sets: true,
            walk_reachability: true,
            max_dirs: 50_000,
            max_entries_per_dir: 32_768,
            orphan_sample_limit: 4,
            fsinfo_tolerance_percent: 2,
        };
        let rep = self.check_with(&opt)?;
        if rep.has_error() {
            return Err(FsCheckerError::Invalid("FsInvalid run check_all"));
        }
        Ok(())
    }
}

/* ========================= Helpers ========================= */

/// Deep walk des chaînes FAT (détection boucles/overflow/indices invalides)
fn check_fat_chains_deep<IO: BlockIO + ?Sized>(io: &mut IO, meta: &Fat32Meta) -> FsCheckerResult {
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

        while (FAT_FIRST_CLUSTER..FAT_EOC).contains(&current) {
            if current < first_cluster || current >= last_cluster {
                return Err(FsCheckerError::Invalid("Cluster out of range in FAT chain"));
            }
            if is_visited(&visited_bitmap, first_cluster, current) {
                return Err(FsCheckerError::Invalid("Loop detected in FAT chain"));
            }

            mark_visited(&mut visited_bitmap, first_cluster, current);

            let next = read_fat_entry(io, meta, current, 0)?;
            chain_len += 1;
            if chain_len > meta.cluster_count as usize {
                return Err(FsCheckerError::Invalid("Invalid FAT chain length"));
            }
            if next == FAT_EOC {
                break;
            }
            current = next;
        }
    }

    Ok(())
}

fn sample_fat<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    sample: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    if sample == 0 {
        return Ok(());
    }

    let count = meta.cluster_count.max(1);
    let step = (count / sample.max(1)).max(1);
    let mut bad = 0u32;
    let mut checked = 0u32;

    let start = FAT_FIRST_CLUSTER;
    let end = FAT_FIRST_CLUSTER + count - 1;

    let mut c = start;
    while c <= end {
        if let Err(e) = read_fat_entry(io, meta, c, 0) {
            bad += 1;
            rep.push(Finding::warn(
                "FAT.SAMPLE",
                format!("read FAT entry {}: {e:?}", c),
            ));
        }
        checked += 1;
        c = c.saturating_add(step);
    }
    if bad == 0 {
        rep.push(Finding::info(
            "FAT.SAMPLE",
            format!("Sampled {} FAT entries OK", checked),
        ));
    }
    Ok(())
}

fn compare_fat_copies_sampled<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    sample: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    if meta.num_fats < 2 {
        rep.push(Finding::info("FAT.MIRROR", "Single FAT (no mirror)"));
        return Ok(());
    }
    let count = meta.cluster_count.max(1);
    let step = (count / sample.max(1)).max(1);
    let mut mismatches = 0u32;
    let mut checked = 0u32;

    let start = FAT_FIRST_CLUSTER;
    let end = FAT_FIRST_CLUSTER + count - 1;

    let mut c = start;
    while c <= end {
        let mut a = [0u8; 4];
        let mut b = [0u8; 4];
        io.read_at(meta.fat_entry_offset(c, 0), &mut a)
            .map_err(FsCheckerError::IO)?;
        if let Err(e) = io.read_at(meta.fat_entry_offset(c, 1), &mut b) {
            rep.push(Finding::warn(
                "FAT.MIRROR",
                format!("FAT#1 read fail @{}: {e:?}", c),
            ));
            return Ok(());
        }
        if a != b {
            mismatches += 1;
            if mismatches <= 4 {
                rep.push(Finding::err(
                    "FAT.MIRROR",
                    format!(
                        "Mismatch @cluster {} (fat0={:08X} fat1={:08X})",
                        c,
                        u32::from_le_bytes(a),
                        u32::from_le_bytes(b)
                    ),
                ));
            }
        }
        checked += 1;
        c = c.saturating_add(step);
    }
    if mismatches == 0 {
        rep.push(Finding::info(
            "FAT.MIRROR",
            format!("FAT copies match on {} sampled entries", checked),
        ));
    }
    Ok(())
}

/* -------- FSINFO consistency (optionnel) -------- */

fn check_fsinfo_consistency<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    tol_percent: u8,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let fsinfo: Fat32FsInfo = {
        // la même fonction read_struct que plus haut n'est pas dispo ici
        // → on lit à la main car on a besoin du secteur exact
        // (mais on sait que FAT_FSINFO_SECTOR pointe au bon LBA)
        // On réutilise le FromBytes:
        let mut buf = [0u8; FAT_MAX_SECTOR_SIZE];
        let bps = meta.bytes_per_sector as usize;
        io.read_at(
            FAT_FSINFO_SECTOR * meta.bytes_per_sector as u64,
            &mut buf[..bps],
        )
        .map_err(FsCheckerError::IO)?;
        Fat32FsInfo::read_from_bytes(&buf[..bps])
            .map_err(|_| FsCheckerError::Invalid("Invalid FSINFO"))?
    };

    let advertised = fsinfo.free_cluster_count;
    if advertised == 0xFFFF_FFFF {
        rep.push(Finding::warn(
            "FSI.FREE",
            "FSINFO.free_count unknown (0xFFFFFFFF)",
        ));
        return Ok(());
    }

    // Mesure réelle (scan entier FAT) — O(n)
    let mut free_measured: u32 = 0;
    let start = 2u32;
    let end = start + meta.cluster_count - 1;
    let mut c = start;

    while c <= end {
        let e = read_fat_entry(io, meta, c, 0)?;
        if e == 0 {
            free_measured += 1;
        }
        c += 1;
    }

    let diff = if advertised > free_measured {
        advertised - free_measured
    } else {
        free_measured - advertised
    };
    let tol = ((meta.cluster_count as u64 * tol_percent as u64) / 100) as u32;

    if diff <= tol {
        rep.push(Finding::info(
            "FSI.CONS",
            format!(
                "FSINFO.free_count ~= measured (adv={} meas={} diff={} tol={})",
                advertised, free_measured, diff, tol
            ),
        ));
    } else {
        rep.push(Finding::warn(
            "FSI.CONS",
            format!(
                "FSINFO.free_count off (adv={} meas={} diff>{})",
                advertised, free_measured, tol
            ),
        ));
    }
    Ok(())
}

/* -------- Walk de l’arbre + LFN + Reachability -------- */

#[derive(Default)]
struct WalkStats {
    dirs: usize,
    entries: usize,
}

fn report_orphans<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    rep: &mut VerifyReport,
    reachable: &[bool],
    sample_limit: usize,
) -> FsCheckerResult<()> {
    let start = FAT_FIRST_CLUSTER;
    let end_incl = FAT_FIRST_CLUSTER + meta.cluster_count - 1;

    let mut samples = 0usize;
    let mut orphans = 0usize;

    let mut c = start;
    while c <= end_incl {
        let e = read_fat_entry(io, meta, c, 0)?;
        let used = e != 0;
        let reach = reachable.get(idx_of(c)).copied().unwrap_or(false);

        if used && !reach {
            orphans += 1;
            if samples < sample_limit {
                rep.push(Finding::warn("FAT.ORPHAN", format!("Orphan cluster {}", c)));
                samples += 1;
            }
        }
        c += 1;
    }

    if orphans == 0 {
        rep.push(Finding::info("FAT.ORPHAN", "No orphan clusters"));
    } else {
        rep.push(Finding::warn(
            "FAT.ORPHAN",
            format!("{} orphan clusters (sampled {})", orphans, samples),
        ));
    }
    Ok(())
}

fn walk_tree_and_validate<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    opt: &Fat32CheckOptions,
    rep: &mut VerifyReport,
    reachable: &mut [bool],
    stats: &mut WalkStats,
) -> FsCheckerResult<()> {
    let mut stack: Vec<u32> = vec![meta.root_unit()];
    let mut seen_dirs = 0usize;

    while let Some(dir_clus) = stack.pop() {
        seen_dirs += 1;
        if seen_dirs > opt.max_dirs {
            rep.push(Finding::warn("DIR.LIMIT", "Directory scan limit reached"));
            break;
        }

        // 1) Marquer tout le répertoire (sa chaîne) en reachable, par RUNS
        {
            let mut cursor = ClusterCursor::new(meta, dir_clus);
            cursor
                .for_each_run(io, |_io, start, len| {
                    // mark [start..start+len)
                    let s = idx_of(start);
                    let e = s + len as usize;
                    if e <= reachable.len() {
                        for i in s..e {
                            reachable[i] = true;
                        }
                        Ok(())
                    } else {
                        Err(FsCursorError::Other("reachable_oob"))
                    }
                })
                .map_err(FsCheckerError::Cursor)?;
        }

        // 2) Analyse de contenu du répertoire (cluster par cluster)
        {
            let mut cursor = ClusterCursor::new(meta, dir_clus);
            while let Some(res) = cursor.next_with(io) {
                let cl = res.map_err(FsCheckerError::Cursor)?;
                let seen_eod =
                    analyze_dir_cluster(io, meta, cl, opt, rep, &mut stack, stats, reachable)?;
                if seen_eod {
                    break;
                }
            }
        }
    }

    stats.dirs = seen_dirs;
    Ok(())
}

fn analyze_dir_cluster<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    clus: u32,
    opt: &Fat32CheckOptions,
    rep: &mut VerifyReport,
    stack: &mut Vec<u32>,
    stats: &mut WalkStats,
    reachable: &mut [bool],
) -> FsCheckerResult<bool> {
    let cs = meta.unit_size();

    // LFN accumulées entre entrées SFN; on les garde à travers les runs
    let mut lfn_stack: Vec<[u8; 32]> = Vec::new();
    let mut entries_this_dir = 0usize;

    // Itération par RUNS (I/O groupées)
    let mut cur = ClusterCursor::new(meta, clus);
    let mut hit_eod = false;

    cur.for_each_run(io, |io, run_start, run_len| {
        let total = (run_len as usize) * cs;
        let mut data = vec![0u8; total];
        let off0 = meta.unit_offset(run_start);
        io.read_block_best_effort(off0, &mut data, total)?;

        for chunk in data.chunks_exact(32) {
            let first = chunk[0];
            if first == FAT_EOD {
                lfn_stack.clear();
                hit_eod = true;
                break; // fin logique du répertoire
            }
            if first == FAT_ENTRY_DELETED {
                lfn_stack.clear();
                continue;
            }

            let attr = chunk[11];

            // LFN piece
            if attr == Fat32Attributes::LFN.bits() {
                if opt.check_lfn_sets {
                    lfn_stack.push(chunk.try_into().unwrap());
                }
                continue;
            }

            // Volume label → ignorer
            if attr & Fat32Attributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            // "." et ".." → ignorer
            let name11 = &chunk[0..11];
            if (attr & Fat32Attributes::DIRECTORY.bits() != 0)
                && (name11 == FAT_DOT_NAME || name11 == FAT_DOTDOT_NAME)
            {
                lfn_stack.clear();
                continue;
            }

            // Valider le set LFN accumulé contre le SFN courant (si demandé)
            if opt.check_lfn_sets && !lfn_stack.is_empty() {
                if let Err(msg) = validate_lfn_set(&lfn_stack, chunk) {
                    rep.push(Finding::err("LFN.SET", msg));
                }
                lfn_stack.clear();
            }

            // Compteurs + plafond
            entries_this_dir += 1;
            stats.entries += 1;
            if entries_this_dir > opt.max_entries_per_dir {
                rep.push(Finding::warn(
                    "DIR.ENTLIMIT",
                    "Too many entries in directory (limit)",
                ));
                // on stoppe le run et la fonction (on ne mark pas plus loin)
                hit_eod = false;
                break;
            }

            // Répertoire enfant ?
            if (attr & Fat32Attributes::DIRECTORY.bits()) != 0 {
                let fst_lo = u16::from_le_bytes([chunk[26], chunk[27]]) as u32;
                let fst_hi = u16::from_le_bytes([chunk[20], chunk[21]]) as u32;
                let child_cluster = (fst_hi << 16) | fst_lo;

                if child_cluster >= FAT_FIRST_CLUSTER && child_cluster != clus {
                    // Si déjà marqué reachable (tête du dir), n’empile pas
                    let idx = idx_of(child_cluster);
                    if idx >= reachable.len() || !reachable[idx] {
                        stack.push(child_cluster);
                    }
                }
                continue;
            }

            // Fichier → marquer reachable sur toute la chaîne (par runs)
            let fst_lo = u16::from_le_bytes([chunk[26], chunk[27]]) as u32;
            let fst_hi = u16::from_le_bytes([chunk[20], chunk[21]]) as u32;
            let first_cluster = (fst_hi << 16) | fst_lo;

            if first_cluster >= FAT_FIRST_CLUSTER {
                let mut fc = ClusterCursor::new_safe(meta, first_cluster);
                fc.for_each_run(io, |_io, start, len| {
                    let s = idx_of(start);
                    let e = s + len as usize;
                    if e <= reachable.len() {
                        for i in s..e {
                            reachable[i] = true;
                        }
                        Ok(())
                    } else {
                        Err(FsCursorError::Other("reachable_oob"))
                    }
                })?;
            }
        }

        // si on a vu EOD ou dépassé le plafond, on sort des runs aussi
        if hit_eod || entries_this_dir > opt.max_entries_per_dir {
            return Ok(());
        }
        Ok(())
    })
    .map_err(FsCheckerError::Cursor)?;

    Ok(hit_eod)
}

fn compare_vbr_main_backup<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let bps = meta.bytes_per_sector as usize;
    let mut main = vec![0u8; bps];
    let mut bak = vec![0u8; bps];
    io.read_at(FAT_VBR_SECTOR * meta.bytes_per_sector as u64, &mut main)
        .map_err(FsCheckerError::IO)?;
    io.read_at(
        FAT_VBR_BACKUP_SECTOR * meta.bytes_per_sector as u64,
        &mut bak,
    )
    .map_err(FsCheckerError::IO)?;

    // Optionnel: neutraliser champs volatils si tu en as; sinon compare brut
    if main == bak {
        rep.push(Finding::info("VBR.MIRROR", "Backup VBR = Main"));
    } else {
        rep.push(Finding::warn("VBR.MIRROR", "Backup VBR ≠ Main"));
    }
    Ok(())
}

/* -------- LFN: ordre + checksum SFN -------- */

fn validate_lfn_set(lfns: &[[u8; 32]], sfn: &[u8]) -> Result<(), String> {
    if lfns.is_empty() {
        return Ok(());
    }

    // Sur disque: [n|0x40, n-1, ..., 1] puis SFN
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
        let expect = n - i as u8; // n, n-1, ..., 1
        if ord != expect {
            return Err(format!(
                "LFN order mismatch (got {}, expect {})",
                ord, expect
            ));
        }
        if i > 0 && (raw[0] & 0x40) != 0 {
            return Err("LFN last-bit set on non-last piece".into());
        }
    }

    let chk = lfn_checksum(&sfn[0..11]);
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

#[inline(always)]
fn idx_of(c: u32) -> usize {
    (c - FAT_FIRST_CLUSTER) as usize
}
