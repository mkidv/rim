// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec;

pub use crate::core::checker::*;
use crate::core::fat::chain as fat_chain;
use crate::fs::fat32::meta::*;
use rimio::prelude::*;

mod boot;
mod fat;
mod walker;

// --- check options (identical or light) ---
#[derive(Clone, Debug)]
pub struct Fat32CheckOptions {
    pub phases: VerifyPhases,
    pub fail_fast: bool,
    pub fat_sample: u32,
    pub deep_fat_walk: bool,
    pub compare_fat_copies: bool,
    pub check_fsinfo_consistency: bool,
    pub check_lfn_sets: bool,
    pub walk_reachability: bool,
    pub max_dirs: usize,
    pub max_entries_per_dir: usize,
    pub orphan_sample_limit: usize,
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
            check_fsinfo_consistency: false, // heavy → off by default
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

pub struct Fat32Checker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Fat32Meta,
}
impl<'a, IO: RimIO + ?Sized> Fat32Checker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Fat32Meta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for Fat32Checker<'a, IO> {
    type Options = Fat32CheckOptions;

    fn check_boot(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        boot::check_boot(self.io, self.meta, rep)?;
        if opt.check_fsinfo_consistency {
            boot::check_fsinfo_consistency(self.io, self.meta, opt.fsinfo_tolerance_percent, rep)?;
        }

        Ok(())
    }

    fn check_chain(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        if opt.fat_sample > 0 {
            fat::fat_sample(self.io, self.meta, opt.fat_sample, rep)?;
        }
        if opt.compare_fat_copies {
            // keep existing sampling or factorize into a small local helper
            fat::compare_fat_copies(self.io, self.meta, opt.fat_sample.max(256), rep)?;
        }
        if opt.deep_fat_walk {
            match fat::deep_walk(self.io, self.meta) {
                Ok(()) => rep.push(Finding::info("FAT.DEEP", "FAT chain walk OK")),
                Err(e) => rep.push(Finding::err("FAT.DEEP", format!("FAT chain walk: {e}"))),
            }
        }

        Ok(())
    }

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let bps = self.meta.bytes_per_sector as usize;
        let spc = self.meta.sectors_per_cluster as usize;
        let mut buf = vec![0u8; bps * spc];
        self.io
            .read_at(self.meta.unit_offset(self.meta.root_unit()), &mut buf)
            .map_err(FsCheckerError::IO)?;
        rep.push(Finding::info("ROOT.IO", "Root cluster readable"));

        if fat_chain::read_entry(self.io, self.meta, self.meta.root_unit(), 0)? == 0 {
            rep.push(Finding::err("ROOT.FAT", "FAT[root] == FREE (0)"));
        }

        Ok(())
    }

    fn check_cross_reference(
        &mut self,
        opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        if opt.walk_reachability {
            let mut walker = walker::Fat32Walker::new(self.io, self.meta);
            let mut stats = walker::WalkerStats::default();

            walker.walk_from_root(opt.check_lfn_sets, rep, &mut stats)?;

            rep.push(Finding::info(
                "DIR.WALK",
                format!(
                    "Walked {} dirs, {} entries (LFN checked: {})",
                    stats.dirs_visited, stats.entries_scanned, opt.check_lfn_sets
                ),
            ));

            walker.report_orphans(rep, opt.orphan_sample_limit)?;
        }
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
