// SPDX-License-Identifier: MIT

//! Generic filesystem consistency verification framework.

mod types;

pub use types::{
    Finding, ReportDisplay, ReportDisplayOpts, Severity, VerifierOptionsLike, VerifyPhases,
    VerifyReport,
};

pub mod stats;
pub use stats::WalkerStats;

pub mod tracker;
pub use tracker::ReachabilityTracker;

pub use crate::errors::{FsCheckerError, FsCheckerResult};

/// Trait for verifying the integrity of a filesystem.
///
/// This trait is typically implemented for each specific filesystem (e.g. FAT32, EXT4)
/// to perform internal consistency checks (VBR validity, superblock, FAT chains, inodes, etc.).
pub trait FsChecker {
    type Options: VerifierOptionsLike + Default;

    #[must_use = "check result must be examined"]
    fn check_with(&mut self, opt: &Self::Options) -> FsCheckerResult<VerifyReport> {
        let mut rep = VerifyReport::default();
        let phases = [
            (
                VerifyPhases::BOOT,
                Self::check_boot
                    as fn(&mut Self, &Self::Options, &mut VerifyReport) -> FsCheckerResult<()>,
            ),
            (VerifyPhases::GEOMETRY, Self::check_geometry),
            (VerifyPhases::CHAIN, Self::check_chain),
            (VerifyPhases::ROOT, Self::check_root),
            (VerifyPhases::CROSSREF, Self::check_cross_reference),
            (VerifyPhases::CONTENT, Self::check_content),
            (VerifyPhases::CUSTOM, Self::check_custom),
        ];
        for (phase, check_fn) in phases {
            if opt.fail_fast() && rep.has_error() {
                break;
            }
            self.run_phase(opt, &mut rep, phase, check_fn)?;
        }
        Ok(rep)
    }

    #[must_use = "check result must be examined"]
    fn check_all(&mut self) -> FsCheckerResult<VerifyReport> {
        self.check_with(&Self::Options::default())
    }

    #[must_use = "check result must be examined"]
    fn fast_check(&mut self) -> FsCheckerResult {
        Ok(())
    }

    fn check_boot(&mut self, _opt: &Self::Options, _rep: &mut VerifyReport) -> FsCheckerResult<()> {
        Ok(())
    }
    fn check_geometry(
        &mut self,
        _opt: &Self::Options,
        _rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        Ok(())
    }
    fn check_chain(
        &mut self,
        _opt: &Self::Options,
        _rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        Ok(())
    }
    fn check_root(&mut self, _opt: &Self::Options, _rep: &mut VerifyReport) -> FsCheckerResult<()> {
        Ok(())
    }
    fn check_cross_reference(
        &mut self,
        _opt: &Self::Options,
        _rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        Ok(())
    }
    fn check_content(
        &mut self,
        _opt: &Self::Options,
        _rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        Ok(())
    }
    fn check_custom(
        &mut self,
        _opt: &Self::Options,
        _rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        Ok(())
    }

    fn run_phase<F>(
        &mut self,
        opt: &Self::Options,
        rep: &mut VerifyReport,
        phase: VerifyPhases,
        f: F,
    ) -> FsCheckerResult<()>
    where
        F: Fn(&mut Self, &Self::Options, &mut VerifyReport) -> FsCheckerResult<()>,
    {
        if opt.fail_fast() && rep.has_error() {
            return Ok(());
        }
        if opt.phases().contains(phase) {
            f(self, opt, rep)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::types::CoreVerifyOptions;

    struct DummyChecker {
        boot_ran: bool,
        geom_ran: bool,
    }

    impl FsChecker for DummyChecker {
        type Options = CoreVerifyOptions;

        fn check_boot(
            &mut self,
            _opt: &Self::Options,
            rep: &mut VerifyReport,
        ) -> FsCheckerResult<()> {
            self.boot_ran = true;
            rep.push(Finding::err("BOOT_ERR", "boot error"));
            Ok(())
        }

        fn check_geometry(
            &mut self,
            _opt: &Self::Options,
            rep: &mut VerifyReport,
        ) -> FsCheckerResult<()> {
            self.geom_ran = true;
            rep.push(Finding::err("GEOM_ERR", "geometry error"));
            Ok(())
        }
    }

    #[test]
    fn test_checker_fail_fast_stops_subsequent_phases() {
        // Without fail_fast: both phases should run
        let mut checker = DummyChecker {
            boot_ran: false,
            geom_ran: false,
        };
        let opt_no_ff = CoreVerifyOptions {
            phases: VerifyPhases::ALL,
            fail_fast: false,
        };
        let rep = checker.check_with(&opt_no_ff).unwrap();
        assert!(checker.boot_ran);
        assert!(checker.geom_ran);
        assert_eq!(rep.count(Severity::Error), 2);

        // With fail_fast: geometry phase must be skipped after boot error
        let mut checker_ff = DummyChecker {
            boot_ran: false,
            geom_ran: false,
        };
        let opt_ff = CoreVerifyOptions {
            phases: VerifyPhases::ALL,
            fail_fast: true,
        };
        let rep_ff = checker_ff.check_with(&opt_ff).unwrap();
        assert!(checker_ff.boot_ran);
        assert!(
            !checker_ff.geom_ran,
            "Subsequent phase must NOT run when fail_fast is enabled"
        );
        assert_eq!(rep_ff.count(Severity::Error), 1);
    }
}
