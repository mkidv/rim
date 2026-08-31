// SPDX-License-Identifier: MIT
use crate::{constant::*, group_layout::GroupLayout, meta::ExtMeta};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, vec, vec::Vec};
use rimio::prelude::*;

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
            let layout = GroupLayout::compute(self.meta, group);

            // Read this group's inode allocation bitmap.
            let bitmap_offset = layout.inode_bitmap_block * self.meta.block_size as u64;

            if let Err(e) = self.io.read_at(bitmap_offset, &mut inode_bitmap) {
                rep.push(Finding::warn(
                    "WALK.IO",
                    format!("Failed reading inode bitmap group {group}: {e:?}"),
                ));
                continue;
            }

            let valid_inodes = self.meta.group_total_inodes(group as usize);

            for i in 0..valid_inodes {
                let byte = inode_bitmap[i / 8];
                let mask = 1u8 << (i % 8);

                // Free inode: nothing to inspect.
                if byte & mask == 0 {
                    continue;
                }

                let inode_num = group * self.meta.inodes_per_group + i as u32 + 1;

                let inode_offset = layout.inode_table_block * self.meta.block_size as u64
                    + i as u64 * self.meta.inode_size as u64;

                if let Err(e) = self.io.read_at(inode_offset, &mut inode_buf) {
                    rep.push(Finding::warn(
                        "WALK.IO",
                        format!("Failed reading inode {inode_num}: {e:?}"),
                    ));
                    continue;
                }

                let i_mode = u16::from_le_bytes(inode_buf[0..2].try_into().unwrap());

                let i_links = u16::from_le_bytes(inode_buf[26..28].try_into().unwrap());

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
                    let i_size = u32::from_le_bytes(inode_buf[4..8].try_into().unwrap());

                    let i_blocks = u32::from_le_bytes(inode_buf[28..32].try_into().unwrap());

                    let i_flags = u32::from_le_bytes(inode_buf[32..36].try_into().unwrap());

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

    /// Deep-scans every inode-table entry and cross-checks it against the bitmap.
    /// This is intentionally O(total inode count) and can be very expensive on large filesystems.
    #[allow(dead_code)]
    pub fn scan_inodes_deep(
        &mut self,
        rep: &mut VerifyReport,
        stats: &mut WalkerStats,
    ) -> FsCheckerResult<()> {
        let inode_size = self.meta.inode_size as usize;

        let mut inode_bitmap = vec![0u8; self.meta.block_size as usize];

        let table_bytes = self.meta.inodes_per_group as usize * inode_size;
        let mut table_buf = vec![0u8; table_bytes];

        for group in 0..self.meta.group_count {
            let layout = GroupLayout::compute(self.meta, group);

            let bitmap_offset = layout.inode_bitmap_block * self.meta.block_size as u64;

            if let Err(e) = self.io.read_at(bitmap_offset, &mut inode_bitmap) {
                rep.push(Finding::warn(
                    "WALK.IO",
                    format!("Failed reading inode bitmap group {group}: {e:?}"),
                ));
                continue;
            }

            let table_offset = layout.inode_table_block * self.meta.block_size as u64;

            if let Err(e) = self.io.read_at(table_offset, &mut table_buf) {
                rep.push(Finding::warn(
                    "WALK.IO",
                    format!("Failed reading inode table group {group}: {e:?}"),
                ));
                continue;
            }

            let first_inode = group as u64 * self.meta.inodes_per_group as u64;

            let remaining = self.meta.inode_count.saturating_sub(first_inode);

            let valid_inodes = remaining.min(self.meta.inodes_per_group as u64) as u32;

            for i in 0..valid_inodes {
                let inode_num = group * self.meta.inodes_per_group + i + 1;

                let bitmap_byte = inode_bitmap[(i / 8) as usize];
                let bitmap_mask = 1u8 << (i % 8);
                let allocated = (bitmap_byte & bitmap_mask) != 0;

                let buf_off = i as usize * inode_size;
                let inode_buf = &table_buf[buf_off..buf_off + inode_size];

                let i_mode = u16::from_le_bytes(inode_buf[0..2].try_into().unwrap());

                let i_links = u16::from_le_bytes(inode_buf[26..28].try_into().unwrap());

                /*
                 * Deliberately use OR here.
                 *
                 * For a free inode, either a non-zero mode or links count means
                 * stale/live-looking metadata remains in an inode marked free.
                 */
                let looks_used = i_mode != 0 || i_links != 0;

                match (allocated, looks_used) {
                    (true, true) => {
                        stats.inodes_checked += 1;
                        self.mark_inode_used(inode_num);

                        if i_mode == 0 || i_links == 0 {
                            rep.push(Finding::warn(
                                "INO.ALLOC",
                                format!(
                                    "Allocated inode {inode_num} is partially empty \
                                 (mode=0x{i_mode:04X}, links={i_links})"
                                ),
                            ));
                        }

                        if (i_mode & 0xF000) == 0 {
                            rep.push(Finding::warn(
                                "INO.MODE",
                                format!(
                                    "Inode {inode_num} has invalid mode \
                                 0x{i_mode:04X} (links={i_links})"
                                ),
                            ));
                        }

                        if (i_mode & 0xF000) == 0xA000 {
                            let i_size = u32::from_le_bytes(inode_buf[4..8].try_into().unwrap());

                            let i_blocks =
                                u32::from_le_bytes(inode_buf[28..32].try_into().unwrap());

                            let i_flags = u32::from_le_bytes(inode_buf[32..36].try_into().unwrap());

                            if i_size < 60 {
                                if i_blocks != 0 {
                                    rep.push(Finding::err(
                                        "SYMLINK.FAST",
                                        format!(
                                            "Fast symlink inode {inode_num} has \
                                         i_blocks={i_blocks} (expected 0)"
                                        ),
                                    ));
                                }

                                if (i_flags & EXT_INODE_FLAG_EXTENTS) != 0 {
                                    rep.push(Finding::err(
                                        "SYMLINK.FAST",
                                        format!(
                                            "Fast symlink inode {inode_num} has \
                                         EXTENTS flag set"
                                        ),
                                    ));
                                }
                            } else if i_blocks == 0 {
                                rep.push(Finding::err(
                                    "SYMLINK.SLOW",
                                    format!(
                                        "Slow symlink inode {inode_num} has \
                                     i_blocks=0 (size={i_size})"
                                    ),
                                ));
                            }
                        }
                    }
                    (true, false) => {
                        stats.inodes_checked += 1;
                        self.mark_inode_used(inode_num);

                        rep.push(Finding::warn(
                            "INO.BITMAP",
                            format!(
                                "Inode {inode_num} is marked allocated but \
                             its inode-table entry is empty"
                            ),
                        ));
                    }
                    (false, true) => {
                        rep.push(Finding::warn(
                            "INO.BITMAP",
                            format!(
                                "Inode {inode_num} is marked free but contains \
                             metadata (mode=0x{i_mode:04X}, links={i_links})"
                            ),
                        ));
                    }
                    (false, false) => {}
                }
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
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
