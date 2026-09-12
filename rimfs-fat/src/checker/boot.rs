// SPDX-License-Identifier: MIT

//! FAT VBR and BPB integrity verification.

use crate::core::{checker::*, fat::*};
use crate::types::FatFsInfo;
use crate::{FsMeta, Validate};
use crate::{constant::*, meta::FatMeta, types::FatVbr};
use rimio::prelude::*;

pub fn check_boot<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let vbr: FatVbr = io.read_struct(FAT_VBR_SECTOR * meta.bytes_per_sector as u64)?;
    match vbr.validate(meta) {
        Ok(()) => rep.push(Finding::info("VBR.OK", "VBR validated")),
        Err(e) => rep.push(Finding::err("VBR.INVALID", e.msg())),
    }
    if meta.bits == 32 {
        boot_compare_main_backup(io, meta, rep)?;
        boot_compare_sector2_backup(io, meta, rep)?;
    }
    boot_geometry_sanity(meta, rep);
    Ok(())
}

pub fn boot_compare_main_backup<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let bps = meta.bytes_per_sector as u64;
    let mut main = vec![0u8; bps as usize];
    let mut bak = vec![0u8; bps as usize];
    io.read_at(FAT_VBR_SECTOR * bps, &mut main)
        .map_err(FsCheckerError::IO)?;
    io.read_at(FAT_VBR_BACKUP_SECTOR * bps, &mut bak)
        .map_err(FsCheckerError::IO)?;
    if main == bak {
        rep.push(Finding::info("VBR.MIRROR", "Backup VBR (Sector 6) = Main"));
    } else {
        rep.push(Finding::warn("VBR.MIRROR", "Backup VBR (Sector 6) ≠ Main"));
    }
    Ok(())
}

pub fn boot_compare_sector2_backup<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let bps = meta.bytes_per_sector as u64;
    let mut main = vec![0u8; bps as usize];
    let mut bak = vec![0u8; bps as usize];
    io.read_at(2 * bps, &mut main).map_err(FsCheckerError::IO)?;
    io.read_at(8 * bps, &mut bak).map_err(FsCheckerError::IO)?;
    if main == bak {
        rep.push(Finding::info(
            "VBR.SEC2_MIRROR",
            "Sector 2 Backup (Sector 8) = Main",
        ));
    } else {
        rep.push(Finding::warn(
            "VBR.SEC2_MIRROR",
            "Sector 2 Backup (Sector 8) ≠ Main",
        ));
    }
    Ok(())
}
pub fn boot_geometry_sanity(meta: &FatMeta, rep: &mut VerifyReport) {
    let bps = meta.bytes_per_sector as usize;
    let spc = meta.sectors_per_cluster as usize;
    if bps == 0 || (bps & (bps - 1)) != 0 {
        rep.push(Finding::err("BPB.BPS", "BytesPerSector not power of two"));
    }
    if spc == 0 || (spc & (spc - 1)) != 0 {
        rep.push(Finding::err(
            "BPB.SPC",
            "SectorsPerCluster not power of two",
        ));
    }
    if meta.num_fats == 0 {
        rep.push(Finding::err("BPB.FATS", "NumberOfFATs == 0"));
    }
    if meta.fat_size_sectors == 0 {
        rep.push(Finding::err("BPB.FATL", "FATLength == 0"));
    }
    if meta.bits == 32 && meta.root_unit() < 2 {
        rep.push(Finding::err("BPB.ROOT", "Root cluster < 2"));
    }
    rep.push(Finding::info(
        "BPB.OK",
        format!(
            "Geometry OK-ish (bps={}, spc={}, fats={}, fat_sectors={})",
            bps, spc, meta.num_fats, meta.fat_size_sectors
        ),
    ));
}

pub fn check_fsinfo_consistency<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
    tol_percent: u8,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let fsi: FatFsInfo = io.read_struct(FAT_FSINFO_SECTOR * meta.bytes_per_sector as u64)?;

    match fsi.validate(meta) {
        Ok(()) => rep.push(Finding::info("FSI.OK", "FSINFO validated")),
        Err(e) => rep.push(Finding::err("FSI.INVALID", e.msg())),
    }

    let advertised = fsi.free_cluster_count.get();
    if advertised == 0xFFFF_FFFF {
        rep.push(Finding::warn(
            "FSI.FREE",
            "FSINFO.free_count unknown (0xFFFFFFFF)",
        ));
    }

    if meta.use_integrity {
        // Stream full FAT and check CRC32
        let fat_size_bytes = meta.fat_size_sectors as u64 * meta.bytes_per_sector as u64;
        let calc = crate::core::utils::checksum_utils::crc32_reader(
            io,
            meta.fat_offset_bytes,
            fat_size_bytes,
        )
        .map_err(FsCheckerError::IO)?;
        let expected = fsi.fat_checksum.get();
        if calc != expected {
            rep.push(Finding::warn(
                "INT.FAT",
                format!("FAT checksum mismatch (headers={expected:08X}, calc={calc:08X})"),
            ));
        } else {
            rep.push(Finding::info("INT.FAT", "FAT checksum validated"));
        }
    }

    if advertised == 0xFFFF_FFFF {
        return Ok(());
    }

    // Real measurement (entire FAT scan) — O(n)
    let (free_measured_scan, _) = FatDriver::new(meta)
        .find_next_free(io)
        .map_err(FsCheckerError::IO)?;
    let free_measured = free_measured_scan as u32;

    let diff = advertised.abs_diff(free_measured);
    let tol = ((meta.cluster_count as u64 * tol_percent as u64) / 100) as u32;

    if diff <= tol {
        rep.push(Finding::info(
            "FSI.CONS",
            format!(
                "FSINFO.free_count ~= measured (adv={advertised} meas={free_measured} diff={diff} tol={tol})"
            ),
        ));
    } else {
        rep.push(Finding::warn(
            "FSI.CONS",
            format!("FSINFO.free_count off (adv={advertised} meas={free_measured} diff>{tol})"),
        ));
    }
    Ok(())
}
