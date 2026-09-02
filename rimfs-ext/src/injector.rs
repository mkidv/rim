// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use core::convert::TryInto;

use crate::core::allocator::FsAllocator;
use crate::{
    core::{
        FsInjectorError, FsInjectorResult,
        injector::*,
        traits::{FileAttributes, NodeKind},
    },
    {
        allocator::{ExtAllocator, ExtHandle},
        constant::*,
        group_layout::GroupLayout,
        meta::ExtMeta,
        ops,
        types::{ExtDirEntry, ExtExtent, ExtInode},
        updates,
    },
};
use rimio::prelude::*;

/// EXT-specific directory context with child subdirectory tracking for link counts
struct ExtContext {
    handle: ExtHandle,
    buf: Vec<u8>,
    /// Number of immediate subdirectories (for parent link count calculation)
    child_dir_count: u16,
    /// Original extent for re-writing the inode
    extent: ExtExtent,
    /// Preserved directory attributes (mode, uid, gid, timestamps)
    attr: FileAttributes,
}

impl ExtContext {
    fn new(handle: ExtHandle, buf: Vec<u8>, extent: ExtExtent, attr: FileAttributes) -> Self {
        Self {
            handle,
            buf,
            child_dir_count: 0,
            extent,
            attr,
        }
    }
}

pub struct ExtInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: ExtAllocator<'a>,
    meta: &'a ExtMeta,
    stack: Vec<ExtContext>,
    /// Track used directories per group for BGDT
    used_dirs_per_group: Vec<u16>,
}

impl<'a, IO: RimIO + ?Sized> ExtInjector<'a, IO> {
    pub fn new(io: &'a mut IO, params: &'a ExtMeta) -> FsInjectorResult<Self> {
        let allocator = ExtAllocator::new(params);
        let group_count = params.block_count.div_ceil(params.blocks_per_group as u64) as usize;
        let mut used_dirs_per_group = vec![0u16; group_count];
        // Account for Root Directory (inode 2) and Lost+Found (inode 11) in Group 0
        if group_count > 0 {
            used_dirs_per_group[0] = 2;
        }

        Ok(Self {
            io,
            allocator,
            meta: params,
            stack: vec![],
            used_dirs_per_group,
        })
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.allocator.blocks.block_offset(block as u64);
        self.io
            .write_block_best_effort(offset, data, self.meta.block_size as usize)?;
        Ok(())
    }

    pub fn flush_metadata(&mut self) -> FsInjectorResult {
        updates::flush_superblock(self.io, &self.allocator, self.meta)?;
        updates::flush_bgdt(
            self.io,
            &self.allocator,
            self.meta,
            &self.used_dirs_per_group,
        )?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<ExtHandle> for ExtInjector<'a, IO> {
    fn set_root_context(&mut self, attr: &FileAttributes) -> FsInjectorResult {
        // Use the pre-formatted root inode (inode 2), not allocating a new one.
        // The root directory was already written by the formatter.

        // First, get the root inode info to find its data block
        let root_inode = EXT_ROOT_INODE;

        // Read existing root directory block (the one written by formatter)
        let layout = GroupLayout::compute(self.meta, 0);
        let root_block = layout.first_data_block;

        // Read existing directory content
        let mut existing = vec![0u8; self.meta.block_size as usize];
        let offset = root_block * self.meta.block_size as u64;
        self.io.read_at(offset, &mut existing)?;

        // Find end of existing entries (look for first rec_len that would exceed block size or inode=0)
        let mut pos = 0usize;
        let mut entries_end = 0usize;
        while pos + 8 <= existing.len() {
            let entry_inode_bytes: [u8; 4] = existing[pos..pos + 4].try_into().unwrap_or([0; 4]);
            let entry_inode = u32::from_le_bytes(entry_inode_bytes);

            let rec_len_bytes: [u8; 2] = existing[pos + 4..pos + 6].try_into().unwrap_or([0; 2]);
            let rec_len = u16::from_le_bytes(rec_len_bytes) as usize;

            if entry_inode == 0 || rec_len == 0 || rec_len > existing.len() - pos {
                entries_end = pos;
                break;
            }
            pos += rec_len;
            entries_end = pos;
        }

        // Keep only the existing entries (. and .. and lost+found)
        existing.truncate(entries_end);

        // Count existing subdirectories for link count
        let mut child_dir_count = 0u16;
        let mut pos = 0usize;
        while pos + 8 <= existing.len() {
            let rec_len = u16::from_le_bytes([existing[pos + 4], existing[pos + 5]]) as usize;
            if rec_len == 0 {
                break;
            }

            let file_type = existing[pos + 7];
            let name_len = existing[pos + 6] as usize;
            let name = &existing[pos + 8..pos + 8 + name_len];

            // Count directories, but ignore "." and ".."
            if file_type == EXT_FT_DIR && name != b"." && name != b".." {
                child_dir_count += 1;
            }
            pos += rec_len;
        }

        // Create handle for root (using existing block, inode 2)
        let mut root_blocks = RunList::new();
        root_blocks.push(Run::new(root_block, 1));
        let handle = ExtHandle::new(root_inode, root_blocks);
        let extent = ExtExtent::new(
            0,
            u32::try_from(root_block)
                .map_err(|_| FsInjectorError::Invalid("EXT root block exceeds extent range"))?,
            1,
        );

        let mut ctx = ExtContext::new(handle, existing, extent, attr.clone());
        ctx.child_dir_count = child_dir_count;
        self.stack.push(ctx);

        // Check if `lost+found` exists
        let mut has_lost_found = false;
        if let Some(ctx) = self.stack.last() {
            let mut pos = 0usize;
            while pos + 8 <= ctx.buf.len() {
                let rec_len = u16::from_le_bytes([ctx.buf[pos + 4], ctx.buf[pos + 5]]) as usize;
                if rec_len == 0 {
                    break;
                }
                let name_len = ctx.buf[pos + 6] as usize;
                let name_start = pos + 8;
                if name_start + name_len <= ctx.buf.len() {
                    let name = &ctx.buf[name_start..name_start + name_len];
                    if name == b"lost+found" {
                        has_lost_found = true;
                        break;
                    }
                }
                pos += rec_len;
            }
        }

        if !has_lost_found {
            let parent = self.stack.last_mut().expect("Root context missing");
            ops::create_lost_found(
                self.io,
                &mut self.allocator,
                self.meta,
                &mut parent.buf,
                &mut parent.child_dir_count,
                &mut self.used_dirs_per_group,
            )?;
        }

        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        // Allocate inode and block for new dir
        let handle = self
            .allocator
            .allocate(self.io, 1)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let block = handle.blocks.0[0].start as u32;

        // Write "." and ".."
        let mut entries = vec![];
        ExtDirEntry::dot(inode).to_raw_buffer(&mut entries);

        // Parent inode
        let parent_inode = self
            .stack
            .last()
            .map(|c| c.handle.inode)
            .unwrap_or(EXT_ROOT_INODE);
        ExtDirEntry::dotdot(parent_inode).to_raw_buffer(&mut entries);

        let extent = ExtExtent::new(0, block, 1);
        let inode_data = ExtInode::from_attr(
            attr,
            self.meta.block_size as u64,
            if attr.is_dir() { 2 } else { 1 },
            self.meta.block_size.div_ceil(512),
            &[extent],
        );
        let inode_buf = inode_data.to_bytes();

        ops::write_inode(self.io, self.meta, inode, &inode_buf)?;

        // Add entry to parent dir
        let entry = ExtDirEntry::from_attr(inode, name, attr);
        if let Some(parent) = self.stack.last_mut() {
            entry.to_raw_buffer(&mut parent.buf);
        }

        // Push new dir context
        let ctx = ExtContext::new(handle, entries, extent, attr.clone());
        self.stack.push(ctx);

        // Track used dir count
        let inode_index = inode - 1;
        let group = (inode_index / self.meta.inodes_per_group) as usize;
        if let Some(count) = self.used_dirs_per_group.get_mut(group) {
            *count += 1;
        }

        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        // Allocate inode and blocks
        let total_size = size as u32;
        let block_size = self.meta.block_size;
        let blocks_needed = total_size.div_ceil(block_size) as usize;

        let handle = self
            .allocator
            .allocate(self.io, blocks_needed)
            .map_err(|_| FsInjectorError::Other("Allocation failed"))?;

        let inode = handle.inode;
        let blocks = handle.blocks;

        use crate::core::utils::stream_copy::write_stream_to_run_list;

        // Stream content to disk
        if !blocks.0.is_empty() {
            write_stream_to_run_list(self.io, self.meta, source, &blocks, size)?;
        }

        let inode_data = if self.meta.features.has_extents {
            let mapped_runs = MappedRunList::from_run_list(&blocks, 0);
            let extents: Vec<ExtExtent> = mapped_runs
                .iter()
                .map(|run| ExtExtent::from(*run))
                .collect();

            ExtInode::from_attr(
                attr,
                total_size as u64,
                if attr.is_dir() { 2 } else { 1 },
                (blocks.total_units() as u32) * (block_size.div_ceil(512)),
                &extents,
            )
        } else {
            use crate::utils::block_map::build_block_map;
            let blocks_vec = blocks.to_units();
            let map = build_block_map(self.io, &mut self.allocator, self.meta, &blocks_vec)?;

            ExtInode::from_attr_block_map(
                attr,
                total_size as u64,
                if attr.is_dir() { 2 } else { 1 },
                (blocks.total_units() as u32) * (block_size.div_ceil(512)),
                &map,
            )
        };
        let inode_buf = inode_data.to_bytes();

        ops::write_inode(self.io, self.meta, inode, &inode_buf)?;

        // Add entry to current dir
        let entry = ExtDirEntry::from_attr(inode, name, attr);
        if let Some(ctx) = self.stack.last_mut() {
            entry.to_raw_buffer(&mut ctx.buf);
        }

        Ok(())
    }

    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let target_bytes = target.as_bytes();
        let target_len = target_bytes.len();

        let mut symlink_attr = attr.clone();
        symlink_attr.kind = NodeKind::Symlink;

        if target_len < 60 {
            // Fast symlink (< 60 bytes): target stored in i_block, 0 data blocks allocated
            let handle = self
                .allocator
                .allocate(self.io, 0)
                .map_err(|_| FsInjectorError::Other("Inode allocation failed"))?;
            let inode = handle.inode;

            let inode_data = ExtInode::new_fast_symlink(&symlink_attr, target);
            let inode_buf = inode_data.to_bytes();
            ops::write_inode(self.io, self.meta, inode, &inode_buf)?;

            let entry = ExtDirEntry::from_attr(inode, name, &symlink_attr);
            if let Some(ctx) = self.stack.last_mut() {
                entry.to_raw_buffer(&mut ctx.buf);
            }
        } else {
            // Slow symlink (>= 60 bytes): allocate data block(s) and stream target payload
            let block_size = self.meta.block_size;
            let blocks_needed = (target_len as u32).div_ceil(block_size) as usize;

            let handle = self
                .allocator
                .allocate(self.io, blocks_needed)
                .map_err(|_| FsInjectorError::Other("Block allocation failed"))?;
            let inode = handle.inode;
            let blocks = handle.blocks;

            let mut target_data = target_bytes.to_vec();
            let mut target_io = rimio::prelude::MemRimIO::new(&mut target_data);
            use crate::core::utils::stream_copy::write_stream_to_run_list;
            write_stream_to_run_list(
                self.io,
                self.meta,
                &mut target_io,
                &blocks,
                target_len as u64,
            )?;

            let blocks_512 = (blocks.total_units() as u32) * (block_size.div_ceil(512));

            let inode_data = if self.meta.features.has_extents {
                let mapped_runs = MappedRunList::from_run_list(&blocks, 0);
                let extents: Vec<ExtExtent> = mapped_runs
                    .iter()
                    .map(|run| ExtExtent::from(*run))
                    .collect();

                ExtInode::from_attr(&symlink_attr, target_len as u64, 1, blocks_512, &extents)
            } else {
                use crate::utils::block_map::build_block_map;
                let blocks_vec = blocks.to_units();
                let map = build_block_map(self.io, &mut self.allocator, self.meta, &blocks_vec)?;

                ExtInode::from_attr_block_map(&symlink_attr, target_len as u64, 1, blocks_512, &map)
            };

            let inode_buf = inode_data.to_bytes();
            ops::write_inode(self.io, self.meta, inode, &inode_buf)?;

            let entry = ExtDirEntry::from_attr(inode, name, &symlink_attr);
            if let Some(ctx) = self.stack.last_mut() {
                entry.to_raw_buffer(&mut ctx.buf);
            }
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            // Pad directory block so last entry spans to end
            ops::pad_directory_block(&mut ctx.buf, self.meta.block_size as usize);
            // Write to first block. Logic limitation: directory size <= 1 block
            if let Some(run) = ctx.handle.blocks.0.first() {
                self.write_block(run.start as u32, &ctx.buf)?;
            }

            // Re-write this directory's inode with correct link count and PRESERVED attributes
            let links = 2 + ctx.child_dir_count;
            let inode_data = ExtInode::from_attr(
                &ctx.attr,
                self.meta.block_size as u64,
                links,
                self.meta.block_size.div_ceil(512),
                &[ctx.extent],
            );
            ops::write_inode(self.io, self.meta, ctx.handle.inode, &inode_data.to_bytes())?;

            // Increment parent's child_dir_count (this dir is a subdirectory of parent)
            if let Some(parent) = self.stack.last_mut() {
                parent.child_dir_count += 1;
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        // Drain the stack by calling flush_current repeatedly
        // This ensures each directory gets its link count updated correctly
        while !self.stack.is_empty() {
            self.flush_current()?;
        }

        self.flush_metadata()?;
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::checker::ExtChecker;
    use crate::formatter::ExtFormatter;
    use crate::meta::ExtFeatureSet;
    use crate::prelude::*;
    use crate::resolver::ExtResolver;
    use rimfs_core::testing::{
        ExpectedFile, ExpectedLink, assert_files, assert_no_errors, assert_symlinks, file_with_attr,
    };

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    fn test_injector_scenario(meta: ExtMeta, _name: &str) {
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let large_content = vec![0xEEu8; 15 * 4096];

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Dir {
                    name: "subdir".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::new_file("hello.txt", b"Hello World!".to_vec())],
                },
                FsNode::new_file("large.bin", large_content.clone()),
            ],
        };

        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        let mut resolver = ExtResolver::new(&mut io, &meta);
        assert_files(
            &mut resolver,
            &[
                ExpectedFile {
                    path: "/subdir/hello.txt",
                    bytes: b"Hello World!",
                },
                ExpectedFile {
                    path: "/large.bin",
                    bytes: &large_content,
                },
            ],
        );
    }

    #[test]
    fn test_ext_variants() {
        test_injector_scenario(ExtMeta::new_ext2(SIZE_BYTES, Some("EXT2")).unwrap(), "Ext2");
        test_injector_scenario(ExtMeta::new_ext3(SIZE_BYTES, Some("EXT3")).unwrap(), "Ext3");
        test_injector_scenario(ExtMeta::new(SIZE_BYTES, Some("EXT")).unwrap(), "Ext");

        let mut features = ExtFeatureSet::EXT;
        features.has_extents = false;
        features.has_64bit = true;
        let exotic_meta =
            ExtMeta::new_custom(features, SIZE_BYTES, Some("EXOTIC"), None, 4096, 8192).unwrap();
        test_injector_scenario(exotic_meta, "Exotic (No Extents, 64bit)");
    }

    #[test]
    fn test_ext_dir_attributes_preserved() {
        let meta = ExtMeta::new(SIZE_BYTES, Some("EXT_DIR_ATTR")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let mut custom_dir_attr = FileAttributes::new_dir();
        custom_dir_attr.mode = Some(0o700);
        custom_dir_attr.uid = Some(1001);
        custom_dir_attr.gid = Some(1002);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![FsNode::Dir {
                name: "private".to_string(),
                attr: custom_dir_attr,
                children: vec![FsNode::new_file("secret.txt", b"my secret".to_vec())],
            }],
        };

        injector.inject_tree(&mut tree).unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);
        let read_attr = resolver.read_attributes("/private").unwrap();

        assert_eq!(
            read_attr.mode,
            Some(0o700),
            "Directory mode must be preserved across flush"
        );
        assert_eq!(
            read_attr.uid,
            Some(1001),
            "Directory UID must be preserved across flush"
        );
        assert_eq!(
            read_attr.gid,
            Some(1002),
            "Directory GID must be preserved across flush"
        );
    }

    #[test]
    fn test_ext_symlink_fast_and_slow() {
        let meta = ExtMeta::new(SIZE_BYTES, Some("EXT_SYMLINK")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let short_target = "usr/bin/demo"; // 12 bytes < 60
        let long_target =
            "this/is/a/very/long/symlink/target/path/that/is/at/least/60/bytes/long/for/testing"; // 82 bytes >= 60
        let dangling_target = "nonexistent-target";

        let mut setuid_attr = FileAttributes::new_file();
        setuid_attr.mode = Some(0o4755);

        let mut setgid_dir_attr = FileAttributes::new_dir();
        setgid_dir_attr.mode = Some(0o2775);

        let mut sticky_attr = FileAttributes::new_dir();
        sticky_attr.mode = Some(0o1777);

        let mut no_access_attr = FileAttributes::new_file();
        no_access_attr.mode = Some(0o0000);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Symlink {
                    name: "short-link".to_string(),
                    target: short_target.to_string(),
                    attr: FileAttributes::new_symlink(),
                },
                FsNode::Symlink {
                    name: "long-link".to_string(),
                    target: long_target.to_string(),
                    attr: FileAttributes::new_symlink(),
                },
                FsNode::Symlink {
                    name: "dangling-link".to_string(),
                    target: dangling_target.to_string(),
                    attr: FileAttributes::new_symlink(),
                },
                FsNode::new_file_from_source(
                    "setuid-demo",
                    alloc::boxed::Box::new(rimio::prelude::VecRimIO::new(
                        b"setuid binary".to_vec(),
                    )),
                    setuid_attr,
                ),
                FsNode::Dir {
                    name: "setgid-dir".to_string(),
                    children: vec![],
                    attr: setgid_dir_attr,
                },
                FsNode::Dir {
                    name: "tmp-like".to_string(),
                    children: vec![],
                    attr: sticky_attr,
                },
                FsNode::new_file_from_source(
                    "no-access",
                    alloc::boxed::Box::new(rimio::prelude::VecRimIO::new(b"".to_vec())),
                    no_access_attr,
                ),
            ],
        };

        injector.inject_tree(&mut tree).unwrap();

        let mut checker = ExtChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert_no_errors(&report);

        let mut resolver = ExtResolver::new(&mut io, &meta);
        assert_symlinks(
            &mut resolver,
            &[
                ExpectedLink {
                    path: "/short-link",
                    target: short_target,
                },
                ExpectedLink {
                    path: "/long-link",
                    target: long_target,
                },
                ExpectedLink {
                    path: "/dangling-link",
                    target: dangling_target,
                },
            ],
        );

        let setuid_read = resolver.read_attributes("/setuid-demo").unwrap();
        assert_eq!(setuid_read.mode, Some(0o4755));

        let setgid_read = resolver.read_attributes("/setgid-dir").unwrap();
        assert_eq!(setgid_read.mode, Some(0o2775));

        let tmp_read = resolver.read_attributes("/tmp-like").unwrap();
        assert_eq!(tmp_read.mode, Some(0o1777));

        let no_access_read = resolver.read_attributes("/no-access").unwrap();
        assert_eq!(no_access_read.mode, Some(0o0000));
    }

    #[test]
    fn test_ext_32bit_uid_gid() {
        let meta = ExtMeta::new(SIZE_BYTES, Some("EXT_32BIT_ID")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExtFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

        let mut high_id_attr = FileAttributes::new_file();
        high_id_attr.uid = Some(70000); // 0x11170 -> lo: 0x1170, hi: 0x0001
        high_id_attr.gid = Some(80000); // 0x13880 -> lo: 0x3880, hi: 0x0001
        high_id_attr.mode = Some(0o640);

        let mut high_id_symlink = FileAttributes::new_symlink();
        high_id_symlink.uid = Some(90000);
        high_id_symlink.gid = Some(95000);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                file_with_attr("app.bin", b"app binary", high_id_attr),
                FsNode::Symlink {
                    name: "app.link".to_string(),
                    target: "app.bin".to_string(),
                    attr: high_id_symlink,
                },
            ],
        };

        injector.inject_tree(&mut tree).unwrap();

        let mut resolver = ExtResolver::new(&mut io, &meta);

        let file_attr = resolver.read_attributes("/app.bin").unwrap();
        assert_eq!(file_attr.uid, Some(70000));
        assert_eq!(file_attr.gid, Some(80000));
        assert_eq!(file_attr.mode, Some(0o640));

        let link_attr = resolver.read_attributes("/app.link").unwrap();
        assert_eq!(link_attr.uid, Some(90000));
        assert_eq!(link_attr.gid, Some(95000));
        assert_symlinks(
            &mut resolver,
            &[ExpectedLink {
                path: "/app.link",
                target: "app.bin",
            }],
        );
    }

    #[test]
    fn test_ext2_ext3_symlinks() {
        for (meta, _name) in [
            (
                ExtMeta::new_ext2(SIZE_BYTES, Some("EXT2_SYM")).unwrap(),
                "Ext2",
            ),
            (
                ExtMeta::new_ext3(SIZE_BYTES, Some("EXT3_SYM")).unwrap(),
                "Ext3",
            ),
        ] {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);

            ExtFormatter::new(&mut io, &meta)
                .format(false)
                .expect("Format failed");

            let mut injector = ExtInjector::new(&mut io, &meta).unwrap();

            let short_target = "bin/sh";
            let long_target =
                "var/lib/docker/overlay2/1234567890abcdef1234567890abcdef1234567890abcdef/merged";

            let mut tree = FsNode::Container {
                attr: FileAttributes::new_dir(),
                children: vec![
                    FsNode::Symlink {
                        name: "sh".to_string(),
                        target: short_target.to_string(),
                        attr: FileAttributes::new_symlink(),
                    },
                    FsNode::Symlink {
                        name: "docker".to_string(),
                        target: long_target.to_string(),
                        attr: FileAttributes::new_symlink(),
                    },
                ],
            };

            injector.inject_tree(&mut tree).unwrap();

            let mut checker = ExtChecker::new(&mut io, &meta);
            let report = checker.check_all().unwrap();
            assert_no_errors(&report);

            let mut resolver = ExtResolver::new(&mut io, &meta);
            assert_symlinks(
                &mut resolver,
                &[
                    ExpectedLink {
                        path: "/sh",
                        target: short_target,
                    },
                    ExpectedLink {
                        path: "/docker",
                        target: long_target,
                    },
                ],
            );
        }
    }
}
