// SPDX-License-Identifier: MIT

use crate::meta::IsoMeta;
use crate::types::*;
use rimfs_core::checker::{Finding, FsChecker, FsCheckerResult, VerifierOptionsLike, VerifyReport};
use rimio::RimIO;

/// Configuration options for ISO 9660 image verification.
#[derive(Debug, Clone, Default)]
pub struct IsoCheckerOptions {
    pub verify_boot_catalog: bool,
}
impl VerifierOptionsLike for IsoCheckerOptions {}

/// Validates ISO 9660 volume descriptors, Joliet/Rock Ridge structures, and El Torito boot catalogs.
pub struct IsoChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a IsoMeta,
}

impl<'a, IO: RimIO + ?Sized> IsoChecker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a IsoMeta) -> Self {
        Self { io, _meta: meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for IsoChecker<'a, IO> {
    type Options = IsoCheckerOptions;

    fn check_boot(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let total_size = self.io.total_size().unwrap_or(0);
        if total_size < 17 * ISO_SECTOR_SIZE as u64 {
            rep.push(Finding::err(
                "ISO.SIZE",
                "ISO 9660 image is smaller than the minimum 17 sectors",
            ));
            return Ok(());
        }

        let mut pvd = [0u8; ISO_SECTOR_SIZE];
        self.io.read_at(16 * ISO_SECTOR_SIZE as u64, &mut pvd)?;

        if pvd[0] != VD_PRIMARY || &pvd[1..6] != ISO_STANDARD_ID {
            rep.push(Finding::err(
                "ISO.PVD",
                "Sector 16 is not a valid Primary Volume Descriptor (PVD)",
            ));
            return Ok(());
        }

        let logical_block_size = get_both_u16(&pvd[128..132]);
        if logical_block_size != ISO_SECTOR_SIZE as u16 {
            rep.push(Finding::err(
                "ISO.BLOCK_SIZE",
                "Logical block size in PVD is not 2048 bytes",
            ));
        }

        let volume_space_size = get_both_u32(&pvd[80..88]);
        let max_sectors = (total_size / ISO_SECTOR_SIZE as u64) as u32;
        if volume_space_size > max_sectors + 100 {
            rep.push(Finding::err(
                "ISO.VOLUME",
                "PVD volume space size exceeds physical storage length",
            ));
        }

        let mut lba = 17u64;
        let mut found_terminator = false;

        while lba < 32 && (lba * ISO_SECTOR_SIZE as u64) < total_size {
            let mut vd = [0u8; ISO_SECTOR_SIZE];
            if self
                .io
                .read_at(lba * ISO_SECTOR_SIZE as u64, &mut vd)
                .is_err()
            {
                break;
            }
            if &vd[1..6] != ISO_STANDARD_ID {
                break;
            }

            match vd[0] {
                VD_BOOT_RECORD => {
                    if &vd[7..39] != EL_TORITO_SYS_ID {
                        rep.push(Finding::warn(
                            "ISO.BOOT_SYS",
                            "Boot record does not match standard El Torito identifier",
                        ));
                    }
                    let catalog_lba = u32::from_le_bytes([vd[71], vd[72], vd[73], vd[74]]);
                    if opt.verify_boot_catalog
                        && catalog_lba > 0
                        && (catalog_lba as u64 * ISO_SECTOR_SIZE as u64) < total_size
                    {
                        let mut cat_buf = [0u8; ISO_SECTOR_SIZE];
                        if self
                            .io
                            .read_at(catalog_lba as u64 * ISO_SECTOR_SIZE as u64, &mut cat_buf)
                            .is_ok()
                            && cat_buf[0] == 0x01
                            && cat_buf[30] == 0x55
                            && cat_buf[31] == 0xAA
                        {
                            let mut sum: u16 = 0;
                            for i in 0..16 {
                                let word = u16::from_le_bytes([cat_buf[i * 2], cat_buf[i * 2 + 1]]);
                                sum = sum.wrapping_add(word);
                            }
                            if sum != 0 {
                                rep.push(Finding::err(
                                    "ISO.BOOT_CHECKSUM",
                                    "El Torito validation entry checksum is invalid",
                                ));
                            }
                        }
                    }
                }
                VD_SUPPLEMENTARY => {
                    if &vd[88..91] != JOLIET_ESCAPE_UCS2_LVL3 {
                        rep.push(Finding::warn(
                            "ISO.JOLIET",
                            "Supplementary Volume Descriptor does not use Joliet UCS-2 escape sequence",
                        ));
                    }
                }
                VD_TERMINATOR => {
                    found_terminator = true;
                    break;
                }
                _ => {}
            }
            lba += 1;
        }

        if !found_terminator {
            rep.push(Finding::err(
                "ISO.TERMINATOR",
                "Volume Descriptor Set Terminator was not found",
            ));
        }

        Ok(())
    }

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let total_size = self.io.total_size().unwrap_or(0);
        if total_size < 17 * ISO_SECTOR_SIZE as u64 {
            return Ok(());
        }

        let mut pvd = [0u8; ISO_SECTOR_SIZE];
        self.io.read_at(16 * ISO_SECTOR_SIZE as u64, &mut pvd)?;
        if pvd[0] != VD_PRIMARY || &pvd[1..6] != ISO_STANDARD_ID {
            return Ok(());
        }

        let root_lba = get_both_u32(&pvd[158..166]);
        let root_size = get_both_u32(&pvd[166..174]) as u64;
        if root_lba as u64 * ISO_SECTOR_SIZE as u64 + root_size > total_size {
            rep.push(Finding::err(
                "ISO.ROOT",
                "Root directory extent exceeds image size",
            ));
        }

        Ok(())
    }
}
