// SPDX-License-Identifier: MIT

//! ISO 9660 image structure and volume descriptor checker.

use crate::records::IsoBootValidationEntry;
use rimio::RimReadStructExt;
use zerocopy::FromBytes;

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

        let pvd: IsoVolumeDescriptor = self.io.read_struct(16 * ISO_SECTOR_SIZE as u64)?;

        if pvd.kind != VD_PRIMARY || &pvd.standard_id != ISO_STANDARD_ID {
            rep.push(Finding::err(
                "ISO.PVD",
                "Sector 16 is not a valid Primary Volume Descriptor (PVD)",
            ));
            return Ok(());
        }

        let logical_block_size = pvd.logical_block_size.get()?;
        if logical_block_size != ISO_SECTOR_SIZE as u16 {
            rep.push(Finding::err(
                "ISO.BLOCK_SIZE",
                "Logical block size in PVD is not 2048 bytes",
            ));
        }

        let volume_space_size = pvd.volume_space_size.get()?;
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
                    let boot = IsoBootDescriptor::ref_from_bytes(&vd)
                        .map_err(|_| rimio::RimIOError::Invalid("Invalid ISO boot descriptor"))?;
                    if &boot.system_id != EL_TORITO_SYS_ID {
                        rep.push(Finding::warn(
                            "ISO.BOOT_SYS",
                            "Boot record does not match standard El Torito identifier",
                        ));
                    }
                    let catalog_lba = boot.catalog_lba.get();
                    if opt.verify_boot_catalog
                        && catalog_lba > 0
                        && (catalog_lba as u64 * ISO_SECTOR_SIZE as u64) < total_size
                    {
                        let mut cat_buf = [0u8; ISO_SECTOR_SIZE];
                        if self
                            .io
                            .read_at(catalog_lba as u64 * ISO_SECTOR_SIZE as u64, &mut cat_buf)
                            .is_ok()
                            && let Ok(validation) =
                                IsoBootValidationEntry::ref_from_bytes(&cat_buf[..32])
                            && validation.header_id == 1
                            && validation.key == [0x55, 0xAA]
                        {
                            let sum = validation.checksum_sum();
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

        let pvd: IsoVolumeDescriptor = self.io.read_struct(16 * ISO_SECTOR_SIZE as u64)?;
        if pvd.kind != VD_PRIMARY || &pvd.standard_id != ISO_STANDARD_ID {
            return Ok(());
        }

        let root_lba = pvd.root.header.extent_lba.get()?;
        let root_size = pvd.root.header.data_length.get()? as u64;
        if root_lba as u64 * ISO_SECTOR_SIZE as u64 + root_size > total_size {
            rep.push(Finding::err(
                "ISO.ROOT",
                "Root directory extent exceeds image size",
            ));
        }

        Ok(())
    }
}
