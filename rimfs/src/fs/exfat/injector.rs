// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{BlockIO, BlockIOExt};

use crate::core::{injector::*, resolver::*};

use crate::fs::exfat::upcase::UpcaseHandle;
use crate::fs::exfat::{allocator::*, constant::*, meta::*, ops, types::*, utils};

struct PendingDir {
    name: String,
    attr: FileAttributes,
    first_cluster: u32,
}

pub struct ExFatInjector<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    allocator: &'a mut ExFatAllocator<'a>,
    meta: &'a ExFatMeta,
    upcase: UpcaseHandle,
    stack: Vec<FsContext<ExFatHandle>>,
    pending_dirs: Vec<Option<PendingDir>>,
}

impl<'a, IO: BlockIO + ?Sized> ExFatInjector<'a, IO> {
    pub fn new(io: &'a mut IO, allocator: &'a mut ExFatAllocator<'a>, meta: &'a ExFatMeta) -> Self {
        let upcase = UpcaseHandle::from_io(io, meta).unwrap();

        Self {
            io,
            allocator,
            meta,
            upcase,
            stack: vec![],
            pending_dirs: vec![],
        }
    }

    fn ensure_chain_capacity(
        &mut self,
        handle: &mut ExFatHandle,
        needed: usize,
    ) -> FsInjectorResult {
        if handle.cluster_chain.len() >= needed {
            return Ok(());
        }
        let missing = needed - handle.cluster_chain.len();
        let extra = self.allocator.allocate_chain(missing)?;
        handle.cluster_chain.extend_from_slice(&extra.cluster_chain);
        Ok(())
    }

    fn write_chain_buffer(&mut self, handle: &ExFatHandle, buf: &[u8]) -> FsInjectorResult {
        let cs = self.meta.unit_size();

        if handle.cluster_chain.len() > 1 {
            let offsets: Vec<u64> = handle
                .cluster_chain
                .iter()
                .map(|&c| self.meta.unit_offset(c))
                .collect();

            let mut full = vec![0u8; handle.cluster_chain.len() * cs];
            full[..buf.len()].copy_from_slice(buf);
            self.io.write_multi_at(&offsets, cs, &full)?;
        } else {
            let c = handle.cluster_chain[0];
            self.io
                .write_block_best_effort(self.meta.unit_offset(c), buf, cs)?;
        }

        // mettre à jour FAT + bitmap pour toute la chaîne
        ops::write_fat_chain(self.io, self.meta, &handle.cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &handle.cluster_chain)?;
        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsNodeInjector<ExFatHandle> for ExFatInjector<'a, IO> {
    fn set_root_context(&mut self, _: &FsNode) -> FsInjectorResult {
        let offset = self.meta.unit_offset(self.meta.root_unit());

        let mut buf = vec![0u8; self.meta.unit_size()];
        self.io.read_at(offset, &mut buf)?;

        // Find the last non-empty entry to determine where to start adding new entries
        // Keep existing entries (Volume Label, Allocation Bitmap, Upcase Table, etc.)
        let eod_pos = buf
            .chunks(32)
            .position(|entry| entry[0] == EXFAT_EOD)
            .unwrap_or(buf.len() / 32);

        // Truncate to remove the end-of-directory marker and any trailing empty entries
        buf.truncate(eod_pos * 32);

        let handle = ExFatHandle::new(self.meta.root_unit());
        self.stack.push(FsContext::new(handle, buf));
        self.pending_dirs.push(None);
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let handle = self.allocator.allocate_unit()?;
        ops::write_fat_chain(self.io, self.meta, &handle.cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &handle.cluster_chain)?;

        // ouvrir le contexte enfant vide
        self.stack.push(FsContext::new(handle.clone(), vec![]));

        self.pending_dirs.push(Some(PendingDir {
            name: name.to_string(),
            attr: attr.clone(),
            first_cluster: handle.cluster_id,
        }));
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        content: &[u8],
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let cs = self.meta.unit_size();
        let need = content.len().div_ceil(cs).max(1);

        let handle = self.allocator.allocate_chain(need)?;

        self.write_chain_buffer(&handle, content)?;

        if let Some(ctx) = self.stack.last_mut() {
            let entry = if is_contiguous_chain(&handle.cluster_chain) {
                ExFatEntries::file_contiguous(
                    name,
                    handle.cluster_id,
                    content.len() as u32,
                    attr,
                    &self.upcase,
                )
            } else {
                ExFatEntries::file(
                    name,
                    handle.cluster_id,
                    content.len() as u32,
                    attr,
                    &self.upcase,
                )
            };

            entry.to_raw_buffer(&mut ctx.buf);
        }
        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            // Check if the last entry is an EOD marker
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != EXFAT_EOD {
                ExFatEodEntry::new().to_raw_buffer(&mut ctx.buf);
            }

            let cs = self.meta.unit_size();
            let used = ctx.buf.len();
            let need_clusters = used.div_ceil(cs).max(1);

            self.ensure_chain_capacity(&mut ctx.handle, need_clusters)?;
            self.write_chain_buffer(&ctx.handle, &ctx.buf)?;

            let pending = self.pending_dirs.pop().unwrap_or(None);

            if let Some(pd) = pending
                && let Some(parent) = self.stack.last_mut()
            {
                let bytes_used = ctx.buf.len() as u64;
                let cluster_size = self.meta.unit_size();
                // arrondi au cluster supérieur
                let data_len = bytes_used.div_ceil(cluster_size as u64) * cluster_size as u64;

                ExFatEntries::dir_with_len(
                    &pd.name,
                    pd.first_cluster,
                    &pd.attr,
                    data_len,
                    &self.upcase,
                )
                .to_raw_buffer(&mut parent.buf);
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        while let Some(mut ctx) = self.stack.pop() {
            // Check if the last entry is an EOD marker
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != EXFAT_EOD {
                ExFatEodEntry::new().to_raw_buffer(&mut ctx.buf);
            }

            let cs = self.meta.unit_size();
            let used = ctx.buf.len();
            let need_clusters = used.div_ceil(cs).max(1);

            self.ensure_chain_capacity(&mut ctx.handle, need_clusters)?;
            self.write_chain_buffer(&ctx.handle, &ctx.buf)?;

            let pending = self.pending_dirs.pop().unwrap_or(None);

            if let Some(pd) = pending
                && let Some(parent) = self.stack.last_mut()
            {
                let bytes_used = ctx.buf.len() as u64;
                let cluster_size = self.meta.unit_size();
                // arrondi au cluster supérieur
                let data_len = bytes_used.div_ceil(cluster_size as u64) * cluster_size as u64;

                ExFatEntries::dir_with_len(
                    &pd.name,
                    pd.first_cluster,
                    &pd.attr,
                    data_len,
                    &self.upcase,
                )
                .to_raw_buffer(&mut parent.buf);
            }
        }
        self.io.flush()?;
        Ok(())
    }
}

fn is_contiguous_chain(chain: &[u32]) -> bool {
    if chain.is_empty() {
        return true;
    }
    let start = chain[0];
    chain
        .iter()
        .enumerate()
        .all(|(i, &c)| c == start + i as u32)
}

#[cfg(test)]
mod tests {
    use crate::fs::exfat::prelude::*;

    #[test]
    fn test_exfat_injector() {
        const SIZE_MB: u64 = 32;
        const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
        let meta = ExFatMeta::new(SIZE_BYTES, Some("TESTFS"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];

        let mut io = MemBlockIO::new(&mut buf);

        // Format the filesystem first
        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut allocator = ExFatAllocator::new(&meta);
        let mut injector = ExFatInjector::new(&mut io, &mut allocator, &meta);

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::Dir {
                    name: "subdir".to_string(),
                    attr: FileAttributes::new_dir(),
                    children: vec![FsNode::File {
                        name: "hello.txt".to_string(),
                        content: b"Hello World!".to_vec(),
                        attr: FileAttributes::new_file(),
                    }],
                },
                FsNode::File {
                    name: "readme.md".to_string(),
                    content: b"Test Readme".to_vec(),
                    attr: FileAttributes::new_file(),
                },
            ],
        };

        injector.inject_tree(&tree).unwrap();

        // Debug: hexdump the root directory
        let mut root_data = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.root_unit()), &mut root_data)
            .unwrap();
        println!("Root directory after injection:");
        for (i, chunk) in root_data[..512].chunks(16).enumerate() {
            print!("{:04X}: ", i * 16);
            for b in chunk {
                print!("{b:02X} ");
            }
            print!(" | ");
            for &b in chunk {
                let c = if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                };
                print!("{c}");
            }
            println!();
        }

        let mut checker = ExFatChecker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        // Debug: Test read_dir directly on root
        let mut parser = ExFatResolver::new(&mut io, &meta);
        let root_entries = parser.read_dir("/").expect("read_dir failed");
        println!("Root entries found by read_dir: {:?}", root_entries);

        let mut parser_back = ExFatResolver::new(&mut io, &meta);
        let mut parsed_tree = parser_back.parse_tree("/*").expect("parse_tree failed");

        tree.sort_children_recursively();
        parsed_tree.sort_children_recursively();

        println!("{tree}");
        println!("{parsed_tree}");
        assert_eq!(tree, parsed_tree);
    }
}
