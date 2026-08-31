use crate::FsMeta;
use crate::core::checker::{Finding, VerifyReport};

use crate::core::fat::FatFsMeta;
// SPDX-License-Identifier: MIT
use crate::constant::*;
use crate::core::{errors::*, fat::*};
use crate::meta::FatMeta;
use rimio::prelude::*;

pub fn fat_sample<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
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

    let mut driver = FatDriver::new(meta);
    let mut c = start;
    while c <= end {
        if let Err(e) = driver.read_entry(io, c) {
            bad += 1;
            rep.push(Finding::warn(
                "FAT.SAMPLE",
                format!("read FAT entry {c}: {e:?}"),
            ));
        }
        checked += 1;
        c = c.saturating_add(step);
    }
    if bad == 0 {
        rep.push(Finding::info(
            "FAT.SAMPLE",
            format!("Sampled {checked} FAT entries OK"),
        ));
    }
    Ok(())
}

pub fn compare_fat_copies<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &FatMeta,
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

    let mut driver = FatDriver::new(meta);
    let mut c = start;
    while c <= end {
        let val0 = driver
            .read_entry_from_table(io, 0, c)
            .map_err(FsCheckerError::IO)?;
        let val1 = driver
            .read_entry_from_table(io, 1, c)
            .map_err(FsCheckerError::IO)?;

        if val0 != val1 {
            mismatches += 1;
            if mismatches <= 4 {
                rep.push(Finding::err(
                    "FAT.MIRROR",
                    format!("Mismatch @cluster {c} (fat0={val0:08X} fat1={val1:08X})",),
                ));
            }
        }
        checked += 1;
        c = c.saturating_add(step);
    }
    if mismatches == 0 {
        rep.push(Finding::info(
            "FAT.MIRROR",
            format!("FAT copies match on {checked} sampled entries"),
        ));
    }
    Ok(())
}
pub fn deep_walk<IO: RimIO + ?Sized>(io: &mut IO, meta: &FatMeta) -> FsCheckerResult<()> {
    let first = meta.first_data_unit();
    let last = meta.last_data_unit();

    let span = (last - first + 1) as usize;
    let mut visited = vec![0u8; span.div_ceil(8)];

    #[inline(always)]
    fn mark(v: &mut [u8], base: u32, c: u32) {
        let i = (c - base) as usize;
        v[i / 8] |= 1 << (i % 8);
    }

    #[inline(always)]
    fn seen(v: &[u8], base: u32, c: u32) -> bool {
        let i = (c - base) as usize;
        (v[i / 8] & (1 << (i % 8))) != 0
    }

    let mut view = FatDriver::new(meta);

    for start in first..=last {
        if seen(&visited, first, start) {
            continue;
        }

        let mut cur = start;
        let mut len = 0usize;

        while cur >= FAT_FIRST_CLUSTER && !meta.is_eoc(cur) {
            if cur < first || cur > last {
                return Err(FsCheckerError::Invalid("Cluster out of range in FAT chain"));
            }

            if seen(&visited, first, cur) {
                return Err(FsCheckerError::Invalid("Loop detected in FAT chain"));
            }

            mark(&mut visited, first, cur);

            let next = view.get(io, cur)?;
            len += 1;

            if len > meta.cluster_count as usize {
                return Err(FsCheckerError::Invalid("Invalid FAT chain length"));
            }

            if next == 0 || meta.is_eoc(next) {
                break;
            }

            cur = next;
        }
    }

    Ok(())
}
