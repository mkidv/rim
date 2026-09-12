// SPDX-License-Identifier: MIT

//! ext2/3/4 filesystem integrity and consistency checker.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use crate::core::checker::*;
use crate::utils::is_sparse_super_group;
use crate::{constant::*, meta::ExtMeta, types::GroupLayout};
mod walker;

use crate::types::{ExtDirEntryHeader, ExtExtentHeader, ExtInodeHeader, ExtSuperblock};
use rimio::{RimIO, RimReadStructExt};
use zerocopy::FromBytes;

#[derive(Clone, Debug)]
pub struct ExtCheckerOptions {
    pub phases: VerifyPhases,
    pub fail_fast: bool,
    /// Check block bitmaps consistency
    pub check_block_bitmaps: bool,
    /// Check inode bitmaps consistency
    pub check_inode_bitmaps: bool,
    /// Check root directory is accessible
    pub check_root_dir: bool,
    /// Verify superblock backups (sparse super groups)
    pub verify_sb_backups: bool,
}

impl Default for ExtCheckerOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
            check_block_bitmaps: true,
            check_inode_bitmaps: true,
            check_root_dir: true,
            verify_sb_backups: true,
        }
    }
}

impl VerifierOptionsLike for ExtCheckerOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }
    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

pub struct ExtChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExtMeta,
}

impl<'a, IO: RimIO + ?Sized> ExtChecker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExtMeta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for ExtChecker<'a, IO> {
    type Options = ExtCheckerOptions;

    fn check_boot(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        // 1. Check main superblock
        check_superblock(self.io, self.meta, 0, rep)?;

        // 2. Verify superblock backups in sparse super groups
        if opt.verify_sb_backups {
            for group in 1..self.meta.group_count {
                if is_sparse_super_group(group) {
                    check_superblock_backup(self.io, self.meta, group, rep)?;
                }
            }
        }

        Ok(())
    }

    fn check_geometry(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        check_bgdt(self.io, self.meta, rep)?;
        Ok(())
    }

    fn check_root(&mut self, opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        if opt.check_root_dir {
            check_root_inode(self.io, self.meta, rep)?;
        }
        Ok(())
    }

    fn check_cross_reference(
        &mut self,
        opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        let mut walker = walker::ExtWalker::new(self.io, self.meta);
        let mut stats = walker::WalkerStats::default();

        // 1. Scan Inodes (populates used_inodes bitmap)
        walker.scan_inodes(rep, &mut stats)?;

        // 2. Walk Tree (verifies connectivity)
        if opt.check_root_dir {
            walker.walk_from_root(rep, &mut stats)?;
        }

        rep.push(Finding::info(
            "WALK.STATS",
            format!(
                "Checked {} inodes, visited {} dirs, found {} files",
                stats.inodes_checked, stats.dirs_visited, stats.files_found
            ),
        ));

        let mut total_calculated_free_blocks = 0u64;
        let mut total_declared_free_blocks = 0u64;
        let mut all_block_bitmaps_ok = true;

        let mut total_calculated_free_inodes = 0u64;
        let mut total_declared_free_inodes = 0u64;
        let mut all_inode_bitmaps_ok = true;

        for group in 0..self.meta.group_count {
            if opt.check_block_bitmaps {
                match check_block_bitmap(self.io, self.meta, group, rep)? {
                    Some((calc, decl)) => {
                        total_calculated_free_blocks += calc as u64;
                        total_declared_free_blocks += decl as u64;
                    }
                    None => {
                        all_block_bitmaps_ok = false;
                    }
                }
            }
            if opt.check_inode_bitmaps {
                match check_inode_bitmap(self.io, self.meta, group, rep)? {
                    Some((calc, decl)) => {
                        total_calculated_free_inodes += calc as u64;
                        total_declared_free_inodes += decl as u64;
                    }
                    None => {
                        all_inode_bitmaps_ok = false;
                    }
                }
            }
        }

        if (opt.check_block_bitmaps && all_block_bitmaps_ok)
            || (opt.check_inode_bitmaps && all_inode_bitmaps_ok)
        {
            let sb: Result<ExtSuperblock, _> = self.io.read_struct(EXT_SUPERBLOCK_OFFSET);
            match sb {
                Ok(sb) => {
                    if opt.check_block_bitmaps && all_block_bitmaps_ok {
                        let sb_free_blocks = if self.meta.features.has_64bit {
                            (sb.s_free_blocks_count_lo.get() as u64)
                                | ((sb.s_free_blocks_count_hi.get() as u64) << 32)
                        } else {
                            sb.s_free_blocks_count_lo.get() as u64
                        };
                        if total_calculated_free_blocks != sb_free_blocks {
                            rep.push(Finding::err(
                                "SB.FREE_BLOCKS",
                                format!(
                                    "Superblock free blocks mismatch: declared {sb_free_blocks}, calculated {total_calculated_free_blocks}"
                                ),
                            ));
                        }
                        if total_declared_free_blocks != sb_free_blocks {
                            rep.push(Finding::err(
                                "SB.BGDT_FREE_BLOCKS",
                                format!(
                                    "Superblock free blocks mismatch: declared {sb_free_blocks}, sum of BGDT descriptors {total_declared_free_blocks}"
                                ),
                            ));
                        }
                    }

                    if opt.check_inode_bitmaps && all_inode_bitmaps_ok {
                        let sb_free_inodes = sb.s_free_inodes_count.get() as u64;
                        if total_calculated_free_inodes != sb_free_inodes {
                            rep.push(Finding::err(
                                "SB.FREE_INODES",
                                format!(
                                    "Superblock free inodes mismatch: declared {sb_free_inodes}, calculated {total_calculated_free_inodes}"
                                ),
                            ));
                        }
                        if total_declared_free_inodes != sb_free_inodes {
                            rep.push(Finding::err(
                                "SB.BGDT_FREE_INODES",
                                format!(
                                    "Superblock free inodes mismatch: declared {sb_free_inodes}, sum of BGDT descriptors {total_declared_free_inodes}"
                                ),
                            ));
                        }
                    }
                }
                Err(e) => {
                    rep.push(Finding::err(
                        "SB.IO",
                        format!("Failed to read superblock for counter verification: {e:?}"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn fast_check(&mut self) -> FsCheckerResult {
        let opt = ExtCheckerOptions {
            phases: VerifyPhases::BOOT | VerifyPhases::GEOMETRY | VerifyPhases::ROOT,
            fail_fast: true,
            check_block_bitmaps: false,
            check_inode_bitmaps: false,
            check_root_dir: true,
            verify_sb_backups: false,
        };
        let rep = self.check_with(&opt)?;
        if rep.has_error() {
            return Err(FsCheckerError::Invalid("Filesystem invalid, run check_all"));
        }
        Ok(())
    }
}

fn check_superblock<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    _group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let sb_offset = EXT_SUPERBLOCK_OFFSET;
    let sb: ExtSuperblock = io.read_struct(sb_offset).map_err(FsCheckerError::IO)?;

    let magic = sb.s_magic.get();
    if magic != EXT_SUPERBLOCK_MAGIC {
        rep.push(Finding::err(
            "SB.MAGIC",
            format!(
                "Invalid superblock magic: 0x{magic:04X}, expected 0x{EXT_SUPERBLOCK_MAGIC:04X}"
            ),
        ));
        return Ok(());
    }
    rep.push(Finding::info("SB.MAGIC", "Superblock magic OK"));

    let block_count_lo = sb.s_blocks_count_lo.get();
    let block_count_hi = sb.s_blocks_count_hi.get();
    let block_count = block_count_lo as u64 | ((block_count_hi as u64) << 32);
    if block_count != meta.block_count {
        rep.push(Finding::warn(
            "SB.BLOCKS",
            format!(
                "Superblock block_count {} != meta {}",
                block_count, meta.block_count
            ),
        ));
    } else {
        rep.push(Finding::info(
            "SB.BLOCKS",
            format!("Block count OK ({block_count})"),
        ));
    }

    let inode_count = sb.s_inodes_count.get();
    if inode_count as u64 != meta.inode_count {
        rep.push(Finding::warn(
            "SB.INODES",
            format!(
                "Superblock inode_count {} != meta {}",
                inode_count, meta.inode_count
            ),
        ));
    } else {
        rep.push(Finding::info(
            "SB.INODES",
            format!("Inode count OK ({inode_count})"),
        ));
    }

    let free_blocks = sb.s_free_blocks_count_lo.get();
    let free_inodes = sb.s_free_inodes_count.get();
    rep.push(Finding::info(
        "SB.FREE",
        format!("Free: {free_blocks} blocks, {free_inodes} inodes"),
    ));

    let feature_compat = sb.s_feature_compat.get();
    let feature_incompat = sb.s_feature_incompat.get();
    let feature_ro_compat = sb.s_feature_ro_compat.get();

    if feature_incompat & EXT_FEATURE_INCOMPAT_EXTENTS != 0 {
        rep.push(Finding::info("SB.FEAT", "Extents feature enabled"));
    } else {
        rep.push(Finding::warn(
            "SB.FEAT",
            "Extents feature NOT enabled (block maps not fully supported)",
        ));
    }

    rep.push(Finding::info(
        "SB.FEATURES",
        format!(
            "Features: compat=0x{feature_compat:08X}, incompat=0x{feature_incompat:08X}, ro_compat=0x{feature_ro_compat:08X}"
        ),
    ));

    let log_block_size = sb.s_log_block_size.get();
    let Some(actual_block_size) = 1024u32.checked_shl(log_block_size) else {
        rep.push(Finding::err(
            "SB.BLKSZ",
            "Invalid superblock block-size shift",
        ));
        return Ok(());
    };
    if actual_block_size != meta.block_size {
        rep.push(Finding::err(
            "SB.BLKSZ",
            format!(
                "Superblock block_size {} != meta {}",
                actual_block_size, meta.block_size
            ),
        ));
    } else {
        rep.push(Finding::info(
            "SB.BLKSZ",
            format!("Block size OK ({actual_block_size} bytes)"),
        ));
    }

    let blocks_per_group = sb.s_blocks_per_group.get();
    if blocks_per_group != meta.blocks_per_group {
        rep.push(Finding::warn(
            "SB.BPG",
            format!(
                "Superblock blocks_per_group {} != meta {}",
                blocks_per_group, meta.blocks_per_group
            ),
        ));
    }

    let inodes_per_group = sb.s_inodes_per_group.get();
    if inodes_per_group != meta.inodes_per_group {
        rep.push(Finding::warn(
            "SB.IPG",
            format!(
                "Superblock inodes_per_group {} != meta {}",
                inodes_per_group, meta.inodes_per_group
            ),
        ));
    }

    Ok(())
}

fn check_superblock_backup<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let group_start_block =
        meta.first_data_block as u64 + group as u64 * meta.blocks_per_group as u64;
    let sb_offset = group_start_block * meta.block_size as u64;

    let sb: ExtSuperblock = io.read_struct(sb_offset).map_err(FsCheckerError::IO)?;

    let magic = sb.s_magic.get();
    if magic != EXT_SUPERBLOCK_MAGIC {
        rep.push(Finding::warn(
            "SB.BACKUP",
            format!("Group {group} backup superblock magic invalid: 0x{magic:04X}"),
        ));
    } else {
        rep.push(Finding::info(
            "SB.BACKUP",
            format!("Group {group} backup superblock OK"),
        ));
    }

    Ok(())
}

fn check_bgdt<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    for group in 0..meta.group_count {
        let entry =
            crate::utils::read_group_descriptor(io, meta, group).map_err(FsCheckerError::IO)?;
        let block_bitmap = entry.block_bitmap(meta.features.has_64bit);
        let inode_bitmap = entry.inode_bitmap(meta.features.has_64bit);
        let inode_table = entry.inode_table(meta.features.has_64bit);
        let free_blocks = entry.free_blocks_ext(meta.features.has_64bit);
        let free_inodes = entry.free_inodes_ext(meta.features.has_64bit);
        let used_dirs = entry.bg_used_dirs_count_lo.get();

        let group_start =
            meta.first_data_block as u64 + group as u64 * meta.blocks_per_group as u64;
        let group_blocks = meta.group_total_blocks(group as usize) as u64;
        let group_end = group_start + group_blocks;

        let mut errors = Vec::new();

        // Validate block_bitmap location
        if block_bitmap < group_start || block_bitmap >= group_end {
            errors.push(format!("block_bitmap {block_bitmap} out of range"));
        }

        // Validate inode_bitmap location
        if inode_bitmap < group_start || inode_bitmap >= group_end {
            errors.push(format!("inode_bitmap {inode_bitmap} out of range"));
        }

        // Validate inode_table location
        let inode_table_blocks =
            (meta.inodes_per_group * meta.inode_size).div_ceil(meta.block_size);
        if inode_table < group_start || inode_table + inode_table_blocks as u64 > group_end {
            errors.push(format!("inode_table {inode_table} out of range"));
        }

        if errors.is_empty() {
            rep.push(Finding::info(
                "BGDT.GRP",
                format!(
                    "Group {group}: OK (blk_bmp={block_bitmap}, ino_bmp={inode_bitmap}, ino_tbl={inode_table}, free_b={free_blocks}, free_i={free_inodes}, dirs={used_dirs})"
                ),
            ));
        } else {
            for err in errors {
                rep.push(Finding::err("BGDT.GRP", format!("Group {group}: {err}")));
            }
        }
    }

    rep.push(Finding::info(
        "BGDT.OK",
        format!("BGDT validated for {} groups", meta.group_count),
    ));

    Ok(())
}

fn check_root_inode<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    // Root inode is inode 2
    let root_inode = EXT_ROOT_INODE;
    let inode_index = root_inode - 1; // 0-based
    let group = inode_index / meta.inodes_per_group;
    let index_in_group = inode_index % meta.inodes_per_group;

    let layout = GroupLayout::compute(meta, group);
    let inode_table_block = layout.inode_table_block;
    let inode_offset = (inode_table_block * meta.block_size as u64)
        + (index_in_group as u64 * meta.inode_size as u64);

    let mut inode_buf = vec![0u8; meta.inode_size as usize];
    io.read_at(inode_offset, &mut inode_buf)
        .map_err(FsCheckerError::IO)?;

    let (inode, _) = ExtInodeHeader::ref_from_prefix(&inode_buf)
        .map_err(|_| FsCheckerError::Invalid("Truncated root inode header"))?;

    let i_mode = inode.i_mode.get();
    let is_dir = (i_mode & 0xF000) == 0x4000;
    if !is_dir {
        rep.push(Finding::err(
            "ROOT.MODE",
            format!("Root inode mode 0x{i_mode:04X} is not a directory"),
        ));
    } else {
        rep.push(Finding::info(
            "ROOT.MODE",
            format!("Root inode is directory (mode=0o{i_mode:o})"),
        ));
    }

    let i_links = inode.i_links_count.get();
    if i_links < 2 {
        rep.push(Finding::warn(
            "ROOT.LINKS",
            format!("Root inode links_count {i_links} < 2"),
        ));
    } else {
        rep.push(Finding::info(
            "ROOT.LINKS",
            format!("Root inode links OK ({i_links})"),
        ));
    }

    let i_flags = inode.i_flags.get();
    if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
        rep.push(Finding::info("ROOT.EXT", "Root inode uses extents"));

        let (header, _) = ExtExtentHeader::ref_from_prefix(&inode.i_block)
            .map_err(|_| FsCheckerError::Invalid("Truncated root extent header"))?;
        let eh_magic = header.eh_magic.get();
        if eh_magic != EXT_EXTENT_HEADER_MAGIC {
            rep.push(Finding::err(
                "ROOT.EXT",
                format!("Root extent header magic invalid: 0x{eh_magic:04X}"),
            ));
        }
    } else {
        rep.push(Finding::warn(
            "ROOT.EXT",
            "Root inode does not use extents (block maps)",
        ));
    }

    // Try to read root directory data
    if i_flags & EXT_INODE_FLAG_EXTENTS != 0 {
        let mut resolver = crate::resolver::ExtResolver::new(io, meta);
        let extents = resolver
            .read_extents(&inode_buf)
            .map_err(|_| FsCheckerError::Invalid("Invalid root extent tree"))?;
        if let Some(extent) = extents.first()
            && extent.ee_block.get() == 0
            && !extent.is_empty()
            && !extent.is_uninit()
        {
            let root_dir_offset = extent.physical_start() * meta.block_size as u64;
            let mut dir_buf = vec![0u8; meta.block_size as usize];
            io.read_at(root_dir_offset, &mut dir_buf)
                .map_err(FsCheckerError::IO)?;

            let first = ExtDirEntryHeader::from_record(&dir_buf);
            if first.is_some_and(|(h, name)| h.inode.get() == root_inode && name == b".") {
                rep.push(Finding::info("ROOT.DOT", "Root directory '.' entry OK"));
            } else {
                rep.push(Finding::warn(
                    "ROOT.DOT",
                    "Root directory first entry is not '.'",
                ));
            }

            // Count entries
            let mut pos = 0usize;
            let mut entry_count = 0;
            while pos + 8 <= dir_buf.len() {
                let Some((header, _)) = ExtDirEntryHeader::from_record(&dir_buf[pos..]) else {
                    break;
                };
                let rec_len = header.rec_len.get() as usize;
                let entry_inode = header.inode.get();
                if entry_inode != 0 {
                    entry_count += 1;
                }
                pos += rec_len;
            }
            rep.push(Finding::info(
                "ROOT.ENTRIES",
                format!("Root directory has {entry_count} entries"),
            ));
        }
    }

    rep.push(Finding::info("ROOT.IO", "Root inode readable"));

    Ok(())
}

fn check_block_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<Option<(u32, u32)>> {
    let entry = match crate::utils::read_group_descriptor(io, meta, group) {
        Ok(entry) => entry,
        Err(e) => {
            rep.push(Finding::err(
                "BGDT.IO",
                format!("Group {group}: failed to read group descriptor: {e:?}"),
            ));
            return Ok(None);
        }
    };

    let block_bitmap = entry.block_bitmap(meta.features.has_64bit);
    let declared_free = entry.free_blocks_ext(meta.features.has_64bit);

    let group_start = meta.first_data_block as u64 + group as u64 * meta.blocks_per_group as u64;
    let group_blocks = meta.group_total_blocks(group as usize) as u64;
    let group_end = group_start + group_blocks;

    if block_bitmap < group_start || block_bitmap >= group_end {
        rep.push(Finding::err(
            "BGDT.LOCATION",
            format!(
                "Group {group}: block_bitmap {block_bitmap} out of range [{group_start}..{group_end})"
            ),
        ));
        return Ok(None);
    }

    let mut bm_buf = vec![0u8; meta.block_size as usize];
    let bm_offset = block_bitmap * meta.block_size as u64;
    if let Err(e) = io.read_at(bm_offset, &mut bm_buf) {
        rep.push(Finding::err(
            "BMP.IO",
            format!("Group {group}: failed to read block bitmap at block {block_bitmap}: {e:?}"),
        ));
        return Ok(None);
    }

    // Count free blocks in valid range 0..group_blocks.
    // Exclude any padding bits beyond group_blocks.
    let full_bytes = (group_blocks / 8) as usize;
    let remaining_bits = (group_blocks % 8) as usize;

    let mut calculated_free = 0u32;
    for &byte in &bm_buf[..full_bytes] {
        calculated_free += byte.count_zeros();
    }
    if remaining_bits > 0 {
        let last_byte = bm_buf[full_bytes];
        for bit in 0..remaining_bits {
            if last_byte & (1 << bit) == 0 {
                calculated_free += 1;
            }
        }
    }

    // For group 0, check reserved blocks are marked
    if group == 0 {
        let layout = GroupLayout::compute(meta, group);
        let reserved_count = layout.metadata_blocks() as u64;
        let mut missing_reserved = 0usize;
        for bit in 0..reserved_count.min(group_blocks) {
            let byte_idx = (bit / 8) as usize;
            let bit_mask = 1u8 << (bit % 8);
            if bm_buf[byte_idx] & bit_mask == 0 {
                missing_reserved += 1;
            }
        }
        if missing_reserved > 0 {
            rep.push(Finding::warn(
                "BMP.RESV",
                format!("Group {group}: {missing_reserved} reserved blocks not marked in bitmap"),
            ));
        }
    }

    if declared_free != calculated_free {
        rep.push(Finding::err(
            "BMP.FREE_BLOCKS",
            format!(
                "Group {group}: free blocks mismatch: declared {declared_free}, calculated {calculated_free}"
            ),
        ));
    }

    let set_bits = (group_blocks as u32).saturating_sub(calculated_free);
    rep.push(Finding::info(
        "BMP.BLK",
        format!("Group {group}: {set_bits} of {group_blocks} blocks used in bitmap"),
    ));

    Ok(Some((calculated_free, declared_free)))
}

fn check_inode_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &ExtMeta,
    group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<Option<(u32, u32)>> {
    let entry = match crate::utils::read_group_descriptor(io, meta, group) {
        Ok(entry) => entry,
        Err(e) => {
            rep.push(Finding::err(
                "BGDT.IO",
                format!("Group {group}: failed to read group descriptor: {e:?}"),
            ));
            return Ok(None);
        }
    };

    let inode_bitmap = entry.inode_bitmap(meta.features.has_64bit);
    let declared_free = entry.free_inodes_ext(meta.features.has_64bit);

    let group_start = meta.first_data_block as u64 + group as u64 * meta.blocks_per_group as u64;
    let group_blocks = meta.group_total_blocks(group as usize) as u64;
    let group_end = group_start + group_blocks;

    if inode_bitmap < group_start || inode_bitmap >= group_end {
        rep.push(Finding::err(
            "BGDT.LOCATION",
            format!(
                "Group {group}: inode_bitmap {inode_bitmap} out of range [{group_start}..{group_end})"
            ),
        ));
        return Ok(None);
    }

    let mut bm_buf = vec![0u8; meta.block_size as usize];
    let bm_offset = inode_bitmap * meta.block_size as u64;
    if let Err(e) = io.read_at(bm_offset, &mut bm_buf) {
        rep.push(Finding::err(
            "BMP.IO",
            format!("Group {group}: failed to read inode bitmap at block {inode_bitmap}: {e:?}"),
        ));
        return Ok(None);
    }

    let group_inodes = meta.group_total_inodes(group as usize) as u64;

    // Count free inodes in valid range 0..group_inodes.
    // Exclude any padding bits beyond group_inodes.
    let full_bytes = (group_inodes / 8) as usize;
    let remaining_bits = (group_inodes % 8) as usize;

    let mut calculated_free = 0u32;
    for &byte in &bm_buf[..full_bytes] {
        calculated_free += byte.count_zeros();
    }
    if remaining_bits > 0 {
        let last_byte = bm_buf[full_bytes];
        for bit in 0..remaining_bits {
            if last_byte & (1 << bit) == 0 {
                calculated_free += 1;
            }
        }
    }

    if group == 0 {
        let root_bit_idx = (EXT_ROOT_INODE - 1) as usize;
        if root_bit_idx < group_inodes as usize {
            let byte_idx = root_bit_idx / 8;
            let bit_mask = 1u8 << (root_bit_idx % 8);
            if bm_buf[byte_idx] & bit_mask == 0 {
                rep.push(Finding::warn(
                    "BMP.ROOT",
                    "Root inode (2) not marked as used in bitmap",
                ));
            }
        }
    }

    if declared_free != calculated_free {
        rep.push(Finding::err(
            "BMP.FREE_INODES",
            format!(
                "Group {group}: free inodes mismatch: declared {declared_free}, calculated {calculated_free}"
            ),
        ));
    }

    let set_bits = (group_inodes as u32).saturating_sub(calculated_free);
    rep.push(Finding::info(
        "BMP.INO",
        format!("Group {group}: {set_bits} of {group_inodes} inodes used in bitmap"),
    ));

    Ok(Some((calculated_free, declared_free)))
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::core::formatter::FsFormatter;
    use crate::core::testing::expect_error;
    use crate::formatter::ExtFormatter;
    use crate::meta::{ExtFeatureSet, ExtMeta};
    use crate::types::ExtSuperblock;
    use rimio::MemRimIO;
    use rimio::prelude::*;
    use zerocopy::IntoBytes;

    #[test]
    fn test_ext_checker_clean_image_passes() {
        let meta = ExtMeta::new(32 * 1024 * 1024, Some("CLEAN")).unwrap();
        let mut disk = vec![0u8; 32 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);
        ExtFormatter::new(&mut io, &meta).format(false).unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            report.ok(),
            "Clean image should pass checker: {:?}",
            report.findings
        );
    }

    #[test]
    fn test_ext_checker_detects_corrupted_counters() {
        let meta = ExtMeta::new(32 * 1024 * 1024, Some("CORRUPT")).unwrap();
        let mut disk = vec![0u8; 32 * 1024 * 1024];
        {
            let mut io = MemRimIO::new(&mut disk);
            ExtFormatter::new(&mut io, &meta).format(false).unwrap();
        }

        // 1. Corrupt group 0 free blocks in BGDT
        {
            let mut io = MemRimIO::new(&mut disk);
            let offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64;
            let mut desc = crate::utils::read_group_descriptor(&mut io, &meta, 0).unwrap();
            let orig = desc.bg_free_blocks_count_lo.get();
            desc.bg_free_blocks_count_lo = (orig.wrapping_add(5)).into();
            io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
                .unwrap();

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker.check_all().unwrap();
            let err = expect_error(&report, "BMP.FREE_BLOCKS");
            assert!(err.msg.contains("Group 0: free blocks mismatch: declared"));
            assert!(err.msg.contains(&format!("declared {}", orig + 5)));
            assert!(err.msg.contains(&format!("calculated {orig}")));
            desc.bg_free_blocks_count_lo = orig.into();
            io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
                .unwrap();
        }

        // 2. Corrupt group 0 free inodes in BGDT
        {
            let mut io = MemRimIO::new(&mut disk);
            let offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64;
            let mut desc = crate::utils::read_group_descriptor(&mut io, &meta, 0).unwrap();
            let orig = desc.bg_free_inodes_count_lo.get();
            desc.bg_free_inodes_count_lo = (orig.wrapping_add(3)).into();
            io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
                .unwrap();

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker.check_all().unwrap();
            let err = expect_error(&report, "BMP.FREE_INODES");
            assert!(err.msg.contains("Group 0: free inodes mismatch: declared"));
            assert!(err.msg.contains(&format!("declared {}", orig + 3)));
            assert!(err.msg.contains(&format!("calculated {orig}")));
            desc.bg_free_inodes_count_lo = orig.into();
            io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
                .unwrap();
        }

        // 3. Corrupt superblock free blocks
        {
            let mut io = MemRimIO::new(&mut disk);
            let sb: ExtSuperblock = io.read_struct(EXT_SUPERBLOCK_OFFSET).unwrap();
            let orig = sb.s_free_blocks_count_lo.get();
            io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x0C, orig + 10)
                .unwrap();

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker.check_all().unwrap();
            let err = expect_error(&report, "SB.FREE_BLOCKS");
            assert!(err.msg.contains("Superblock free blocks mismatch"));
            assert!(err.msg.contains(&format!("declared {}", orig + 10)));
            assert!(err.msg.contains(&format!("calculated {orig}")));
            io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x0C, orig).unwrap();
        }

        // 4. Corrupt superblock free inodes
        {
            let mut io = MemRimIO::new(&mut disk);
            let sb: ExtSuperblock = io.read_struct(EXT_SUPERBLOCK_OFFSET).unwrap();
            let orig = sb.s_free_inodes_count.get();
            io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x10, orig + 7)
                .unwrap();

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker.check_all().unwrap();
            let err = expect_error(&report, "SB.FREE_INODES");
            assert!(err.msg.contains("Superblock free inodes mismatch"));
            assert!(err.msg.contains(&format!("declared {}", orig + 7)));
            assert!(err.msg.contains(&format!("calculated {orig}")));
            io.write_u32_at(EXT_SUPERBLOCK_OFFSET + 0x10, orig).unwrap();
        }
    }

    #[test]
    fn test_ext_checker_partial_group_accounting() {
        // 12 MB with 1024-byte blocks:
        // blocks_per_group = 8192, total blocks = 12288 (group 0: 8192, group 1: 4096 = partial)
        let meta = ExtMeta::new_custom(
            ExtFeatureSet::EXT2,
            12 * 1024 * 1024,
            Some("PARTIAL"),
            None,
            1024,
            1024,
        )
        .unwrap();
        assert_eq!(meta.group_count, 2);
        assert_eq!(meta.group_total_blocks(0), 8192);
        assert_eq!(meta.group_total_blocks(1), 4096);

        let mut disk = vec![0u8; 12 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);
        ExtFormatter::new(&mut io, &meta).format(false).unwrap();

        // Healthy partial group must pass
        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            report.ok(),
            "Healthy partial group volume should pass checker: {:?}",
            report.findings
        );

        // Corrupt group 1 (partial group) free block counter in BGDT
        let offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64
            + meta.bgdt_entry_size as u64;
        let mut desc = crate::utils::read_group_descriptor(&mut io, &meta, 1).unwrap();
        let orig = desc.bg_free_blocks_count_lo.get();
        desc.bg_free_blocks_count_lo = (orig.wrapping_sub(4)).into();
        io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
            .unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        let err = expect_error(&report, "BMP.FREE_BLOCKS");
        assert!(err.msg.contains("Group 1: free blocks mismatch: declared"));
        assert!(err.msg.contains(&format!("declared {}", orig - 4)));
        assert!(err.msg.contains(&format!("calculated {orig}")));
    }

    #[test]
    fn test_ext_checker_multigroup_accounting() {
        // 16 MB with 1024-byte blocks -> 2 full groups of 8192 blocks each = 16384 blocks
        let meta = ExtMeta::new_custom(
            ExtFeatureSet::EXT,
            16 * 1024 * 1024,
            Some("MULTIGRP"),
            None,
            1024,
            1024,
        )
        .unwrap();
        assert_eq!(meta.group_count, 2);
        assert_eq!(meta.group_total_blocks(0), 8192);
        assert_eq!(meta.group_total_blocks(1), 8192);

        let mut disk = vec![0u8; 16 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);
        ExtFormatter::new(&mut io, &meta).format(false).unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            report.ok(),
            "Healthy multi-group volume should pass: {:?}",
            report.findings
        );

        // Corrupt group 1 descriptor's free block counter in BGDT
        let offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64
            + meta.bgdt_entry_size as u64;
        let mut desc = crate::utils::read_group_descriptor(&mut io, &meta, 1).unwrap();
        let orig = desc.bg_free_blocks_count_lo.get();
        desc.bg_free_blocks_count_lo = (orig + 1).into();
        io.write_at(offset, &desc.as_bytes()[..meta.bgdt_entry_size])
            .unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.code == "BMP.FREE_BLOCKS" && f.msg.contains("Group 1"))
        );
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.code == "SB.BGDT_FREE_BLOCKS")
        );
    }

    #[test]
    fn test_ext_checker_distinguishes_io_error() {
        let meta = ExtMeta::new(32 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0u8; 32 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut disk);
        ExtFormatter::new(&mut io, &meta).format(false).unwrap();

        let desc = crate::utils::read_group_descriptor(&mut io, &meta, 0).unwrap();
        let bm_offset = desc.block_bitmap(meta.features.has_64bit) * meta.block_size as u64;

        use crate::core::testing::FailingRimIO;
        let mut failing_io = FailingRimIO::new(MemRimIO::new(&mut disk)).fail_read_at(bm_offset);
        let mut checker = ExtChecker::new(&mut failing_io, &meta);
        let report = checker.check_all().unwrap();

        assert!(
            report.findings.iter().any(|f| f.code == "BMP.IO"),
            "Expected BMP.IO finding"
        );
        assert!(
            !report.findings.iter().any(|f| f.code == "BMP.FREE_BLOCKS"),
            "Should not report counter mismatch when read failed"
        );
    }
}
