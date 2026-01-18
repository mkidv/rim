// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

use crate::core::checker::*;
use crate::fs::ext4::{constant::*, group_layout::GroupLayout, meta::Ext4Meta};
mod walker;

use rimio::RimIO;

use core::convert::TryInto;

#[derive(Clone, Debug)]
pub struct Ext4CheckOptions {
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

impl Default for Ext4CheckOptions {
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

impl VerifierOptionsLike for Ext4CheckOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }
    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

pub struct Ext4Checker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Ext4Meta,
}

impl<'a, IO: RimIO + ?Sized> Ext4Checker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Ext4Meta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsChecker for Ext4Checker<'a, IO> {
    type Options = Ext4CheckOptions;

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
        // Check Block Group Descriptor Table
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
        let mut walker = walker::Ext4Walker::new(self.io, self.meta);
        let mut stats = walker::WalkerStats::default();

        // 1. Scan Inodes (populates used_inodes bitmap)
        walker.scan_inodes(rep, &mut stats)?;

        // 2. Walk Tree (verifies connectivity)
        #[cfg(feature = "std")]
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

        for group in 0..self.meta.group_count {
            if opt.check_block_bitmaps {
                check_block_bitmap(self.io, self.meta, group, rep)?;
                // Future improvement: Compare walker.used_blocks_bitmap vs implementation
            }
            if opt.check_inode_bitmaps {
                check_inode_bitmap(self.io, self.meta, group, rep)?;
                // Future improvement: Compare walker.used_inodes_bitmap vs implementation
            }
        }
        Ok(())
    }

    fn fast_check(&mut self) -> FsCheckerResult {
        let opt = Ext4CheckOptions {
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

/* =========================================================================
   Superblock Checks
========================================================================= */

fn check_superblock<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Ext4Meta,
    _group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let sb_offset = EXT4_SUPERBLOCK_OFFSET;
    let mut sb_buf = [0u8; EXT4_SUPERBLOCK_SIZE];
    io.read_at(sb_offset, &mut sb_buf)
        .map_err(FsCheckerError::IO)?;

    // Check magic number
    let magic = u16::from_le_bytes(sb_buf[0x38..0x3A].try_into().unwrap());
    if magic != EXT4_SUPERBLOCK_MAGIC {
        rep.push(Finding::err(
            "SB.MAGIC",
            format!(
                "Invalid superblock magic: 0x{magic:04X}, expected 0x{EXT4_SUPERBLOCK_MAGIC:04X}"
            ),
        ));
        return Ok(());
    }
    rep.push(Finding::info("SB.MAGIC", "Superblock magic OK"));

    // Check block count
    let block_count = u32::from_le_bytes(sb_buf[0x04..0x08].try_into().unwrap());
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

    // Check inode count
    let inode_count = u32::from_le_bytes(sb_buf[0x00..0x04].try_into().unwrap());
    if inode_count != meta.inode_count {
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

    // Check free blocks and inodes
    let free_blocks = u32::from_le_bytes(sb_buf[0x0C..0x10].try_into().unwrap());
    let free_inodes = u32::from_le_bytes(sb_buf[0x10..0x14].try_into().unwrap());
    rep.push(Finding::info(
        "SB.FREE",
        format!("Free: {free_blocks} blocks, {free_inodes} inodes"),
    ));

    // Check filesystem features
    let feature_compat = u32::from_le_bytes(sb_buf[0x5C..0x60].try_into().unwrap());
    let feature_incompat = u32::from_le_bytes(sb_buf[0x60..0x64].try_into().unwrap());
    let feature_ro_compat = u32::from_le_bytes(sb_buf[0x64..0x68].try_into().unwrap());

    // Verify extents feature is enabled
    if feature_incompat & EXT4_FEATURE_INCOMPAT_EXTENTS != 0 {
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

    // Check block size
    let log_block_size = u32::from_le_bytes(sb_buf[0x18..0x1C].try_into().unwrap());
    let actual_block_size = 1024u32 << log_block_size;
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

    // Check blocks per group
    let blocks_per_group = u32::from_le_bytes(sb_buf[0x20..0x24].try_into().unwrap());
    if blocks_per_group != meta.blocks_per_group {
        rep.push(Finding::warn(
            "SB.BPG",
            format!(
                "Superblock blocks_per_group {} != meta {}",
                blocks_per_group, meta.blocks_per_group
            ),
        ));
    }

    // Check inodes per group
    let inodes_per_group = u32::from_le_bytes(sb_buf[0x28..0x2C].try_into().unwrap());
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
    meta: &Ext4Meta,
    group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let group_start_block = meta.first_data_block + group * meta.blocks_per_group;
    let sb_offset = group_start_block as u64 * meta.block_size as u64;

    let mut sb_buf = [0u8; EXT4_SUPERBLOCK_SIZE];
    io.read_at(sb_offset, &mut sb_buf)
        .map_err(FsCheckerError::IO)?;

    let magic = u16::from_le_bytes(sb_buf[0x38..0x3A].try_into().unwrap());
    if magic != EXT4_SUPERBLOCK_MAGIC {
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

/* =========================================================================
   BGDT Checks
========================================================================= */

fn check_bgdt<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Ext4Meta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let bgdt_offset = (meta.first_data_block + 1) as u64 * meta.block_size as u64;

    for group in 0..meta.group_count {
        let entry_offset = bgdt_offset + (group as u64 * EXT4_BGDT_ENTRY_SIZE as u64);

        let mut entry = [0u8; EXT4_BGDT_ENTRY_SIZE];
        io.read_at(entry_offset, &mut entry)
            .map_err(FsCheckerError::IO)?;

        let block_bitmap = u32::from_le_bytes(entry[0..4].try_into().unwrap());
        let inode_bitmap = u32::from_le_bytes(entry[4..8].try_into().unwrap());
        let inode_table = u32::from_le_bytes(entry[8..12].try_into().unwrap());
        let free_blocks = u16::from_le_bytes(entry[12..14].try_into().unwrap());
        let free_inodes = u16::from_le_bytes(entry[14..16].try_into().unwrap());
        let used_dirs = u16::from_le_bytes(entry[16..18].try_into().unwrap());

        let group_start = meta.first_data_block + group * meta.blocks_per_group;
        let group_end = group_start + meta.blocks_per_group;

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
            (meta.inodes_per_group * EXT4_DEFAULT_INODE_SIZE).div_ceil(meta.block_size);
        if inode_table < group_start || inode_table + inode_table_blocks > group_end {
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

/* =========================================================================
   Root Inode Check
========================================================================= */

fn check_root_inode<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Ext4Meta,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    // Root inode is inode 2
    let root_inode = EXT4_ROOT_INODE;
    let inode_index = root_inode - 1; // 0-based
    let group = inode_index / meta.inodes_per_group;
    let index_in_group = inode_index % meta.inodes_per_group;

    let layout = GroupLayout::compute(meta, group);
    let inode_table_block = layout.inode_table_block;
    let inode_offset = (inode_table_block as u64 * meta.block_size as u64)
        + (index_in_group as u64 * EXT4_DEFAULT_INODE_SIZE as u64);

    let mut inode_buf = [0u8; EXT4_DEFAULT_INODE_SIZE as usize];
    io.read_at(inode_offset, &mut inode_buf)
        .map_err(FsCheckerError::IO)?;

    // Check mode (should be directory)
    let i_mode = u16::from_le_bytes(inode_buf[0..2].try_into().unwrap());
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

    // Check links count (should be >= 2)
    let i_links = u16::from_le_bytes(inode_buf[26..28].try_into().unwrap());
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

    // Check extents flag
    let i_flags = u32::from_le_bytes(inode_buf[32..36].try_into().unwrap());
    if i_flags & EXT4_INODE_FLAG_EXTENTS != 0 {
        rep.push(Finding::info("ROOT.EXT", "Root inode uses extents"));

        // Check extent header magic
        let eh_magic = u16::from_le_bytes(inode_buf[40..42].try_into().unwrap());
        if eh_magic != EXT4_EXTENT_HEADER_MAGIC {
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
    if i_flags & EXT4_INODE_FLAG_EXTENTS != 0 {
        let eh_entries = u16::from_le_bytes(inode_buf[42..44].try_into().unwrap());
        if eh_entries > 0 {
            // First extent
            let ee_block = u32::from_le_bytes(inode_buf[52..56].try_into().unwrap());
            let ee_len = u16::from_le_bytes(inode_buf[56..58].try_into().unwrap());
            let ee_start_lo = u32::from_le_bytes(inode_buf[60..64].try_into().unwrap());

            if ee_block == 0 && ee_len > 0 {
                // Try to read root dir block
                let root_dir_offset = ee_start_lo as u64 * meta.block_size as u64;
                let mut dir_buf = vec![0u8; meta.block_size as usize];
                io.read_at(root_dir_offset, &mut dir_buf)
                    .map_err(FsCheckerError::IO)?;

                // Check first entry (should be ".")
                let first_inode = u32::from_le_bytes(dir_buf[0..4].try_into().unwrap());
                let first_name_len = dir_buf[6] as usize;
                if first_inode == root_inode && first_name_len == 1 && dir_buf[8] == b'.' {
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
                    let rec_len =
                        u16::from_le_bytes(dir_buf[pos + 4..pos + 6].try_into().unwrap()) as usize;
                    let entry_inode = u32::from_le_bytes(dir_buf[pos..pos + 4].try_into().unwrap());
                    if rec_len == 0 || rec_len > dir_buf.len() - pos {
                        break;
                    }
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
    }

    rep.push(Finding::info("ROOT.IO", "Root inode readable"));

    Ok(())
}

/* =========================================================================
   Bitmap Checks
========================================================================= */

fn check_block_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Ext4Meta,
    group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let layout = GroupLayout::compute(meta, group);
    let block_bitmap_offset = layout.block_bitmap_block as u64 * meta.block_size as u64;

    let bitmap_size = (meta.blocks_per_group / 8) as usize;
    let mut bitmap = vec![0u8; bitmap_size.min(meta.block_size as usize)];
    io.read_at(block_bitmap_offset, &mut bitmap)
        .map_err(FsCheckerError::IO)?;

    // Expected used bits for metadata
    let mut expected_used = alloc::vec::Vec::new();

    // Reserved blocks (superblock, BGDT)
    for i in 0..layout.reserved_blocks {
        expected_used.push(i);
    }

    // Bitmaps
    let _group_start = meta.first_data_block + group * meta.blocks_per_group;
    expected_used.push(layout.block_bitmap_block - _group_start);
    expected_used.push(layout.inode_bitmap_block - _group_start);

    // Inode table
    for i in 0..layout.inode_table_blocks {
        expected_used.push(layout.inode_table_block - _group_start + i);
    }

    // Count set bits
    let mut set_bits = 0u32;
    for byte in &bitmap {
        set_bits += byte.count_ones();
    }

    // For group 0, at minimum check reserved blocks are marked
    if group == 0 {
        let mut missing_reserved = Vec::new();
        for &bit_idx in &expected_used {
            let byte_idx = bit_idx as usize / 8;
            let bit_mask = 1u8 << (bit_idx % 8);
            if byte_idx < bitmap.len() && (bitmap[byte_idx] & bit_mask) == 0 {
                missing_reserved.push(bit_idx);
            }
        }
        if !missing_reserved.is_empty() {
            rep.push(Finding::warn(
                "BMP.RESV",
                format!(
                    "Group {}: {} reserved blocks not marked in bitmap",
                    group,
                    missing_reserved.len()
                ),
            ));
        }
    }

    // Report summary
    let total_blocks = if group == meta.group_count - 1 {
        meta.block_count - group * meta.blocks_per_group
    } else {
        meta.blocks_per_group
    };

    rep.push(Finding::info(
        "BMP.BLK",
        format!("Group {group}: {set_bits} of {total_blocks} blocks used in bitmap"),
    ));

    Ok(())
}

fn check_inode_bitmap<IO: RimIO + ?Sized>(
    io: &mut IO,
    meta: &Ext4Meta,
    group: u32,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let layout = GroupLayout::compute(meta, group);
    let inode_bitmap_offset = layout.inode_bitmap_block as u64 * meta.block_size as u64;

    let bitmap_size = (meta.inodes_per_group / 8) as usize;
    let mut bitmap = vec![0u8; bitmap_size.min(meta.block_size as usize)];
    io.read_at(inode_bitmap_offset, &mut bitmap)
        .map_err(FsCheckerError::IO)?;

    // Count set bits
    let mut set_bits = 0u32;
    for byte in &bitmap {
        set_bits += byte.count_ones();
    }

    // Check reserved inodes in group 0 (inodes 1-10 are reserved)
    if group == 0 {
        // Inodes 1-2 should definitely be in use (bad blocks inode, root inode)
        let root_bit_idx = (EXT4_ROOT_INODE - 1) as usize; // 0-based
        let byte_idx = root_bit_idx / 8;
        let bit_mask = 1u8 << (root_bit_idx % 8);
        if byte_idx < bitmap.len() && (bitmap[byte_idx] & bit_mask) == 0 {
            rep.push(Finding::warn(
                "BMP.ROOT",
                "Root inode (2) not marked as used in bitmap",
            ));
        }
    }

    rep.push(Finding::info(
        "BMP.INO",
        format!(
            "Group {}: {} of {} inodes used in bitmap",
            group, set_bits, meta.inodes_per_group
        ),
    ));

    Ok(())
}

/* =========================================================================
   Helpers
========================================================================= */

/// Check if this group is a sparse super group (0, 1, 3^n, 5^n, 7^n)
fn is_sparse_super_group(group: u32) -> bool {
    if group == 0 || group == 1 {
        return true;
    }

    // Check if power of 3
    let mut n = group;
    while n > 1 && n.is_multiple_of(3) {
        n /= 3;
    }
    if n == 1 {
        return true;
    }

    // Check if power of 5
    let mut n = group;
    while n > 1 && n.is_multiple_of(5) {
        n /= 5;
    }
    if n == 1 {
        return true;
    }

    // Check if power of 7
    let mut n = group;
    while n > 1 && n.is_multiple_of(7) {
        n /= 7;
    }
    n == 1
}
