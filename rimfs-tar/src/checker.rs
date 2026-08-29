// SPDX-License-Identifier: MIT

use crate::meta::TarMeta;
use crate::types::{TAR_BLOCK_SIZE, calculate_checksum, parse_octal};
use rimfs_core::checker::{FsChecker, FsCheckerResult, VerifierOptionsLike, VerifyReport};
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
        let mut offset = 0;
        let mut header = [0u8; TAR_BLOCK_SIZE];

        while self.io.read_at(offset, &mut header).is_ok() {
            if header.iter().all(|&b| b == 0) {
                break;
            }

            let expected_chksum = parse_octal(&header[148..156]) as u32;
            let actual_chksum = calculate_checksum(&header);
            if expected_chksum != actual_chksum {
                rep.push(rimfs_core::checker::Finding::err(
                    "TAR_CHKSUM",
                    "Invalid TAR header checksum",
                ));
                return Ok(());
            }

            let size = parse_octal(&header[124..136]);
            let padded = (size as usize + TAR_BLOCK_SIZE - 1) & !(TAR_BLOCK_SIZE - 1);
            offset += (TAR_BLOCK_SIZE + padded) as u64;
        }

        Ok(())
    }
}
