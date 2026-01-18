use crate::core::checker::{Finding, VerifyReport};
use crate::core::cursor::ClusterMeta;
// SPDX-License-Identifier: MIT
use crate::FsMeta;
use crate::core::{errors::*, fat};
use crate::fs::fat32::constant::*;
use crate::fs::fat32::meta::Fat32Meta;
use rimio::prelude::*;

pub fn fat_sample<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
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

    let mut c = start;
    while c <= end {
        if let Err(e) = fat::chain::read_entry(io, meta, c, 0) {
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
    meta: &Fat32Meta,
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

    let mut c = start;
    while c <= end {
        let mut a = [0u8; 4];
        let mut b = [0u8; 4];
        io.read_at(meta.fat_entry_offset(c, 0), &mut a)
            .map_err(FsCheckerError::IO)?;
        if let Err(e) = io.read_at(meta.fat_entry_offset(c, 1), &mut b) {
            rep.push(Finding::warn(
                "FAT.MIRROR",
                format!("FAT#1 read fail @{c}: {e:?}"),
            ));
            return Ok(());
        }
        if a != b {
            mismatches += 1;
            if mismatches <= 4 {
                rep.push(Finding::err(
                    "FAT.MIRROR",
                    format!(
                        "Mismatch @cluster {} (fat0={:08X} fat1={:08X})",
                        c,
                        u32::from_le_bytes(a),
                        u32::from_le_bytes(b)
                    ),
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

pub fn deep_walk<IO: RimIO + ?Sized>(io: &mut IO, meta: &Fat32Meta) -> FsCheckerResult<()> {
    let first = meta.first_data_unit();
    let last = meta.last_data_unit();
    let span = (last - first) as usize;

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

    for start in first..last {
        if seen(&visited, first, start) {
            continue;
        }
        let mut cur = start;
        let mut len = 0usize;

        while (FAT_FIRST_CLUSTER..FAT_EOC).contains(&cur) {
            if cur < first || cur >= last {
                return Err(FsCheckerError::Invalid("Cluster out of range in FAT chain"));
            }
            if seen(&visited, first, cur) {
                return Err(FsCheckerError::Invalid("Loop detected in FAT chain"));
            }
            mark(&mut visited, first, cur);

            let next = fat::chain::read_entry(io, meta, cur, 0)?;
            len += 1;
            if len > meta.cluster_count as usize {
                return Err(FsCheckerError::Invalid("Invalid FAT chain length"));
            }
            if next == FAT_EOC {
                break;
            }
            cur = next;
        }
    }
    Ok(())
}
