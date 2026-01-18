// SPDX-License-Identifier: MIT
use crate::fs::ext4::{constant::*, group_layout::GroupLayout, meta::Ext4Meta};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, vec, vec::Vec};
use rimio::prelude::*;

use super::{Finding, FsCheckerResult, VerifyReport};
pub use crate::core::checker::stats::WalkerStats;

pub struct Ext4Walker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Ext4Meta,
    pub used_inodes_bitmap: Vec<u8>,
}

impl<'a, IO: RimIO + ?Sized> Ext4Walker<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Ext4Meta) -> Self {
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

    /// Iterates over all inodes in the filesystem (slow but thorough)
    pub fn scan_inodes(
        &mut self,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<()> {
        let inodes_per_group = self.meta.inodes_per_group;
        let inode_size = EXT4_DEFAULT_INODE_SIZE as u64; // simplified

        for group in 0..self.meta.group_count {
            let layout = GroupLayout::compute(self.meta, group);
            let table_blk = layout.inode_table_block;

            // Read entire inode table for the group
            let table_bytes = (inodes_per_group as u64) * inode_size;
            let mut table_buf = vec![0u8; table_bytes as usize];
            let offset = table_blk as u64 * self.meta.block_size as u64;

            if let Err(e) = self.io.read_at(offset, &mut table_buf) {
                rep.push(Finding::warn(
                    "WALK.IO",
                    format!("Failed reading inode table group {group}: {e:?}"),
                ));
                continue;
            }

            for i in 0..inodes_per_group {
                let inode_num = group * inodes_per_group + i + 1;
                // SKIP reserved inodes < 11 (except 2=ROOT) if strict?
                // Actually we just check everything that looks used.

                let buf_off = (i as u64 * inode_size) as usize;
                let inode_buf = &table_buf[buf_off..buf_off + inode_size as usize];

                // Check if inode is in use (mode != 0 or links > 0)
                let i_mode = u16::from_le_bytes(match inode_buf[0..2].try_into() {
                    Ok(arr) => arr,
                    Err(_) => continue,
                });
                let i_links = u16::from_le_bytes(match inode_buf[26..28].try_into() {
                    Ok(arr) => arr,
                    Err(_) => continue,
                });

                if i_mode != 0 && i_links != 0 {
                    stats.inodes_checked += 1;
                    self.mark_inode_used(inode_num);

                    // Collect blocks used by this inode (if possible)
                    // (Simplified: just handle extents or direct blocks if easy)
                    // For thorough check, we should parse extent tree.
                    // For MVP, maybe just valid mode checks.

                    // Basic sanity check
                    if (i_mode & 0xF000) == 0 {
                        rep.push(Finding::warn(
                            "INO.MODE",
                            format!("Inode {inode_num} has Links={i_links} but Mode=0"),
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
        let root = EXT4_ROOT_INODE;

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
            let mut resolver = crate::fs::ext4::resolver::Ext4Resolver::new(self.io, self.meta);
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

                // EXT4_FT_DIR = 2
                if entry.file_type == EXT4_FT_DIR {
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
