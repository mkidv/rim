// SPDX-License-Identifier: MIT

//! ext2/3/4 directory tree and inode reachability walker.

use crate::types::ExtInodeHeader;
use crate::{constant::*, meta::ExtMeta};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, vec, vec::Vec};
use rimio::prelude::*;
use zerocopy::FromBytes;

use super::{Finding, FsCheckerResult, VerifyReport};
pub use crate::core::checker::stats::WalkerStats;

pub struct ExtWalker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExtMeta,
    pub used_inodes_bitmap: Vec<u8>,
}

impl<'a, IO: RimIO + ?Sized> ExtWalker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExtMeta) -> Self {
        let inode_bits = meta.inode_count.div_ceil(8) as usize;
        let _block_bits = meta.block_count.div_ceil(8) as usize;
        Self {
            io,
            meta,
            used_inodes_bitmap: vec![0u8; inode_bits],
        }
    }

    pub fn mark_inode_used(&mut self, inode: u32) {
        let idx = (inode - 1) as usize;
        if idx / 8 < self.used_inodes_bitmap.len() {
            self.used_inodes_bitmap[idx / 8] |= 1 << (idx % 8);
        }
    }

    /// Scans allocated inodes only, using each block group's inode bitmap.
    pub fn scan_inodes(
        &mut self,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<()> {
        let inode_size = self.meta.inode_size as usize;

        // Reused buffers: no allocation per inode/group iteration.
        let mut inode_bitmap = vec![0u8; self.meta.block_size as usize];
        let mut inode_buf = vec![0u8; inode_size];

        for group in 0..self.meta.group_count {
            let desc = match crate::utils::read_group_descriptor(self.io, self.meta, group) {
                Ok(desc) => desc,
                Err(e) => {
                    rep.push(Finding::warn(
                        "WALK.IO",
                        format!("Failed reading descriptor group {group}: {e:?}"),
                    ));
                    continue;
                }
            };

            let bitmap_block = desc.inode_bitmap(self.meta.features.has_64bit);
            let bitmap_offset = bitmap_block * self.meta.block_size as u64;

            if let Err(e) = self.io.read_at(bitmap_offset, &mut inode_bitmap) {
                rep.push(Finding::warn(
                    "WALK.IO",
                    format!("Failed reading inode bitmap group {group}: {e:?}"),
                ));
                continue;
            }

            let valid_inodes = self.meta.group_total_inodes(group as usize);
            let inode_table_block = desc.inode_table(self.meta.features.has_64bit);

            for i in 0..valid_inodes {
                let byte = inode_bitmap[i / 8];
                let mask = 1u8 << (i % 8);

                // Free inode: nothing to inspect.
                if byte & mask == 0 {
                    continue;
                }

                let inode_num = group * self.meta.inodes_per_group + i as u32 + 1;

                let inode_offset = inode_table_block * self.meta.block_size as u64
                    + i as u64 * self.meta.inode_size as u64;

                if let Err(e) = self.io.read_at(inode_offset, &mut inode_buf) {
                    rep.push(Finding::warn(
                        "WALK.IO",
                        format!("Failed reading inode {inode_num}: {e:?}"),
                    ));
                    continue;
                }

                let (inode, _) = ExtInodeHeader::ref_from_prefix(&inode_buf)
                    .map_err(|_| super::FsCheckerError::Invalid("Truncated inode header"))?;
                let i_mode = inode.i_mode.get();

                let i_links = inode.i_links_count.get();

                // Bitmap says allocated, so an empty inode is suspicious.
                if i_mode == 0 || i_links == 0 {
                    rep.push(Finding::warn(
                        "INO.ALLOC",
                        format!(
                            "Allocated inode {inode_num} has mode=0x{i_mode:04X}, links={i_links}"
                        ),
                    ));
                    continue;
                }

                stats.inodes_checked += 1;
                self.mark_inode_used(inode_num);

                // Basic sanity check.
                if (i_mode & 0xF000) == 0 {
                    rep.push(Finding::warn(
                        "INO.MODE",
                        format!(
                            "Inode {inode_num} has Links={i_links} but invalid mode=0x{i_mode:04X}"
                        ),
                    ));
                } else if (i_mode & 0xF000) == 0xA000 {
                    // Symlink validation
                    let i_size = inode.i_size_lo.get();

                    let i_blocks = inode.i_blocks_lo.get();

                    let i_flags = inode.i_flags.get();

                    if i_size < 60 {
                        // Fast symlink
                        if i_blocks != 0 {
                            rep.push(Finding::err(
                            "SYMLINK.FAST",
                            format!(
                                "Fast symlink inode {inode_num} has i_blocks={i_blocks} (expected 0)"
                            ),
                        ));
                        }

                        if (i_flags & EXT_INODE_FLAG_EXTENTS) != 0 {
                            rep.push(Finding::err(
                                "SYMLINK.FAST",
                                format!("Fast symlink inode {inode_num} has EXTENTS flag set"),
                            ));
                        }
                    } else if i_blocks == 0 {
                        rep.push(Finding::err(
                            "SYMLINK.SLOW",
                            format!(
                                "Slow symlink inode {inode_num} has i_blocks=0 (size={i_size})"
                            ),
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    pub fn walk_from_root(
        &mut self,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<()> {
        let root = EXT_ROOT_INODE;

        let mut stack = vec![(root, 0)];
        let mut visited = Vec::new(); // detect loops
        visited.push(root);

        // We need a resolver to read directories.
        // Ideally we should persist it or create it on fly.
        // Creating on fly has overhead of struct creation (cheap).

        while let Some((dir_inode, depth)) = stack.pop() {
            stats.dirs_visited += 1;
            stats.max_depth = stats.max_depth.max(depth);

            if depth > 256 {
                rep.push(Finding::warn(
                    "WALK.DEPTH",
                    "Directory depth limit reached (256)",
                ));
                continue;
            }

            // Use resolver to read entries
            let mut resolver = crate::resolver::ExtResolver::new(self.io, self.meta);
            let entries = match resolver.read_dir_entries(dir_inode) {
                Ok(e) => e,
                Err(e) => {
                    rep.push(Finding::warn(
                        "WALK.DIR",
                        format!("Failed to read dir {dir_inode}: {e:?}"),
                    ));
                    continue;
                }
            };

            // Validate entries
            for entry in entries {
                if entry.name == "." || entry.name == ".." {
                    continue;
                }

                if entry.inode == 0 {
                    continue;
                }

                // EXT_FT_DIR = 2
                if entry.file_type == EXT_FT_DIR {
                    if visited.contains(&entry.inode) {
                        rep.push(Finding::warn(
                            "WALK.LOOP",
                            format!("Loop detected at inode {}", entry.inode),
                        ));
                    } else {
                        visited.push(entry.inode);
                        stack.push((entry.inode, depth + 1));
                    }
                } else {
                    stats.files_found += 1;
                }
            }
        }
        Ok(())
    }
}
