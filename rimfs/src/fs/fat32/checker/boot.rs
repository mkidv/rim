// SPDX-License-Identifier: MIT
use crate::core::{checker::*, fat};
use crate::fs::fat32::types::Fat32FsInfo;
use crate::fs::fat32::{constant::*, meta::Fat32Meta, types::Fat32Vbr};
use crate::{FsMeta, Validate};
use rimio::prelude::*;

pub fn check_boot<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let vbr: Fat32Vbr = io.read_struct(FAT_VBR_SECTOR * meta.bytes_per_sector as u64)?;
    match vbr.validate(meta) {
        Ok(()) => rep.push(Finding::info("VBR.OK", "VBR validated")),
        Err(e) => rep.push(Finding::err("VBR.INVALID", e.msg())),
    }
    boot_compare_main_backup(io, meta, rep)?;
    boot_geometry_sanity(meta, rep);
    Ok(())
}

pub fn boot_compare_main_backup<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let bps = meta.bytes_per_sector as usize;
    let mut main = vec![0u8; bps];
    let mut bak = vec![0u8; bps];
    io.read_at(FAT_VBR_SECTOR * bps as u64, &mut main)
        .map_err(FsCheckerError::IO)?;
    io.read_at(FAT_VBR_BACKUP_SECTOR * bps as u64, &mut bak)
        .map_err(FsCheckerError::IO)?;
    if main == bak {
        rep.push(Finding::info("VBR.MIRROR", "Backup VBR = Main"));
    } else {
        rep.push(Finding::warn("VBR.MIRROR", "Backup VBR ≠ Main"));
    }
    Ok(())
}
pub fn boot_geometry_sanity(meta: &Fat32Meta, rep: &mut VerifyReport) {
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
    if meta.root_unit() < 2 {
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
    meta: &Fat32Meta,
    tol_percent: u8,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let fsi: Fat32FsInfo = io.read_struct(FAT_FSINFO_SECTOR * meta.bytes_per_sector as u64)?;

    match fsi.validate(meta) {
        Ok(()) => rep.push(Finding::info("FSI.OK", "FSINFO validated")),
        Err(e) => rep.push(Finding::err("FSI.INVALID", e.msg())),
    }

    let advertised = fsi.free_cluster_count;
    if advertised == 0xFFFF_FFFF {
        rep.push(Finding::warn(
            "FSI.FREE",
            "FSINFO.free_count unknown (0xFFFFFFFF)",
        ));
        return Ok(());
    }

    // Real measurement (entire FAT scan) — O(n)
    let mut free_measured: u32 = 0;
    let start = 2u32;
    let end = start + meta.cluster_count - 1;
    let mut c = start;

    while c <= end {
        let e = fat::chain::read_entry(io, meta, c, 0)?;
        if e == 0 {
            free_measured += 1;
        }
        c += 1;
    }

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
