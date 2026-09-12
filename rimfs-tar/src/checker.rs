// SPDX-License-Identifier: MIT

//! TAR archive format and block boundary checker.

use crate::meta::TarMeta;
use crate::resolver::TarResolver;
use rimfs_core::checker::{FsChecker, FsCheckerResult, VerifierOptionsLike, VerifyReport};
use rimfs_core::errors::{FsCheckerError, FsResolverError};
use rimio::RimIO;

#[derive(Debug, Clone, Default)]
pub struct TarCheckerOptions;
impl VerifierOptionsLike for TarCheckerOptions {}

/// Validates TAR archive consistency, checksums, and block alignments.
pub struct TarChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a TarMeta,
}

impl<'a, IO: RimIO + ?Sized> TarChecker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> Self {
        Self { io, _meta: meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for TarChecker<'a, IO> {
    type Options = TarCheckerOptions;

    fn check_content(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        if let Err(error) = TarResolver::new(self.io, self._meta).scan_entries(|_, _| {}) {
            if let FsResolverError::IO(error) = error {
                return Err(FsCheckerError::IO(error));
            }
            let code = if matches!(error, FsResolverError::Invalid("TAR checksum mismatch")) {
                "TAR.CHECKSUM"
            } else {
                "TAR.ARCHIVE"
            };
            rep.push(rimfs_core::checker::Finding::err(code, error.msg()));
        }

        Ok(())
    }
}
