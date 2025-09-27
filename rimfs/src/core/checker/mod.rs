// SPDX-License-Identifier: MIT

mod types;

pub use types::{Finding, Severity, VerifierOptionsLike, VerifyPhases, VerifyReport};

pub use crate::core::errors::{FsCheckerError, FsCheckerResult};

/// Trait for verifying the integrity of a filesystem.
///
/// This trait is typically implemented for each specific filesystem (e.g. FAT32, EXT4)
/// to perform internal consistency checks (VBR validity, superblock, FAT chains, inodes, etc.).
pub trait FsChecker {
    type Options: VerifierOptionsLike + Default;

    fn check_with(&mut self, opt: &Self::Options) -> FsCheckerResult<VerifyReport> {
        let mut rep = VerifyReport::default();
        self.run_phase(opt, &mut rep, VerifyPhases::BOOT, Self::check_boot)?;
        self.run_phase(opt, &mut rep, VerifyPhases::GEOMETRY, Self::check_geometry)?;
        self.run_phase(opt, &mut rep, VerifyPhases::CHAIN, Self::check_chain)?;
        self.run_phase(opt, &mut rep, VerifyPhases::ROOT, Self::check_root)?;
        self.run_phase(
            opt,
            &mut rep,
            VerifyPhases::CROSSREF,
            Self::check_cross_reference,
        )?;
        self.run_phase(opt, &mut rep, VerifyPhases::CONTENT, Self::check_content)?;
        self.run_phase(opt, &mut rep, VerifyPhases::CUSTOM, Self::check_custom)?;
        Ok(rep)
    }

    fn check_all(&mut self) -> FsCheckerResult<VerifyReport> {
        self.check_with(&Self::Options::default())
    }

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
        if opt.phases().contains(phase) {
            f(self, opt, rep)?;
            if opt.fail_fast() && rep.has_error() {
                return Ok(());
            }
        }
        Ok(())
    }
}
