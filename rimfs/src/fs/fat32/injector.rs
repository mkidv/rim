// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::{RimIO, RimIOExt};

use crate::core::{fat, injector::*, resolver::*};

use crate::fs::fat32::{allocator::*, constant::*, meta::*, types::*};

pub struct Fat32Injector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: &'a mut Fat32Allocator<'a>,
    meta: &'a Fat32Meta,
    // Stack of open directory contexts (top = current dir)
    stack: Vec<FsContext<Fat32Handle>>,
}

impl<'a, IO: RimIO + ?Sized> Fat32Injector<'a, IO> {
    pub fn new(io: &'a mut IO, allocator: &'a mut Fat32Allocator<'a>, meta: &'a Fat32Meta) -> Self {
        Self {
            io,
            allocator,
            meta,
            stack: vec![],
        }
    }

    fn ensure_chain_capacity(
        &mut self,
        handle: &mut Fat32Handle,
        needed: usize,
    ) -> FsInjectorResult {
        if handle.cluster_chain.len() >= needed {
            return Ok(());
        }
        let missing = needed - handle.cluster_chain.len();
        let extra: Fat32Handle = self.allocator.allocate_chain(missing)?;
        handle.cluster_chain.extend_from_slice(&extra.cluster_chain);
        Ok(())
    }

    fn write_chain_buffer(&mut self, handle: &Fat32Handle, buf: &[u8]) -> FsInjectorResult {
        let cs = self.meta.unit_size();

        if handle.cluster_chain.len() > 1 {
            // Write full chain in one go
            let offsets: Vec<u64> = handle
                .cluster_chain
                .iter()
                .map(|&c| self.meta.unit_offset(c))
                .collect();

            let mut full = vec![0u8; handle.cluster_chain.len() * cs];
            full[..buf.len()].copy_from_slice(buf);
            self.io.write_multi_at(&offsets, cs, &full)?;
        } else {
            // Single-cluster directory/data
            let c = handle.cluster_chain[0];
            self.io
                .write_block_best_effort(self.meta.unit_offset(c), buf, cs)?;
        }

        // Update FAT for the full chain (safe even if already reserved as EOC).
        fat::chain::write_chain::<IO, Fat32Meta>(self.io, self.meta, &handle.cluster_chain)?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsNodeInjector<Fat32Handle> for Fat32Injector<'a, IO> {
    fn set_root_context(&mut self, _: &FsNode) -> FsInjectorResult {
        // Load root cluster’s existing entries, strip trailing EOD region
        let offset = self.meta.unit_offset(self.meta.root_unit());

        let mut buf = vec![0u8; self.meta.unit_size()];
        self.io.read_at(offset, &mut buf)?;

        let eod_pos = buf
            .chunks(32)
            .position(|entry| entry[0] == FAT_EOD)
            .unwrap_or(buf.len() / 32);

        buf.truncate(eod_pos * 32);

        // Ensure the handle’s cluster_id equals the real root cluster (usually 2).
        let handle = Fat32Handle::new(self.meta.root_unit());

        self.stack.push(FsContext::new(handle, buf));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        // Allocate and IMMEDIATELY reserve the child dir’s first cluster in FAT (EOC).
        let handle: Fat32Handle = self.allocator.allocate_unit()?;
        fat::chain::write_chain::<IO, Fat32Meta>(self.io, self.meta, &handle.cluster_chain)?;

        // Resolve parent cluster robustly.
        let parent_cluster = self
            .stack
            .last()
            .map(|ctx| {
                if ctx.handle.cluster_id == self.meta.root_unit() {
                    0
                } else {
                    ctx.handle.cluster_id
                }
            })
            .unwrap_or(self.meta.root_unit());

        // Build child directory head in-memory: "." + ".." + EOD.
        let mut child_buf = Vec::with_capacity(self.meta.unit_size());

        Fat32Entries::dot(handle.cluster_id).to_raw_buffer(&mut child_buf);
        Fat32Entries::dotdot(parent_cluster).to_raw_buffer(&mut child_buf);

        // Append the directory entry into the CURRENT parent now (size = 0).
        if let Some(parent) = self.stack.last_mut() {
            Fat32Entries::dir(name, handle.cluster_id, attr).to_raw_buffer(&mut parent.buf)
        }

        // Push child context (we will write it at flush_current/flush).
        self.stack.push(FsContext::new(handle, child_buf));
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimIO,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        // Allocate content chain and write file data first (best locality).
        let cs = self.meta.unit_size();
        let need = (size as usize).div_ceil(cs).max(1);

        let handle: Fat32Handle = self.allocator.allocate_chain(need)?;

        fat::chain::write_chain::<IO, Fat32Meta>(self.io, self.meta, &handle.cluster_chain)?;

        use crate::core::utils::stream_copy::write_stream_to_units;

        // ... (in write_file) ...
        // Stream content to disk
        if !handle.cluster_chain.is_empty() {
            write_stream_to_units(self.io, self.meta, source, &handle.cluster_chain, size)?;
        }

        // Append the file entry to the CURRENT dir buffer.
        if let Some(ctx) = self.stack.last_mut() {
            Fat32Entries::file(name, handle.cluster_id, size as u32, attr)
                .to_raw_buffer(&mut ctx.buf)
        }
        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        // Write ONLY the current directory buffer; no parent linking here.
        if let Some(mut ctx) = self.stack.pop() {
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != FAT_EOD {
                Fat32EodEntry::new().to_raw_buffer(&mut ctx.buf);
            }

            let cs = self.meta.unit_size();
            let used = ctx.buf.len();
            let need_clusters = used.div_ceil(cs).max(1);

            self.ensure_chain_capacity(&mut ctx.handle, need_clusters)?;
            self.write_chain_buffer(&ctx.handle, &ctx.buf)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        // Drain remaining directory contexts; again, only data writes here.
        while let Some(mut ctx) = self.stack.pop() {
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != FAT_EOD {
                Fat32EodEntry::new().to_raw_buffer(&mut ctx.buf);
            }
            let cs = self.meta.unit_size();
            let used = ctx.buf.len();
            let need_clusters = used.div_ceil(cs).max(1);

            self.ensure_chain_capacity(&mut ctx.handle, need_clusters)?;
            self.write_chain_buffer(&ctx.handle, &ctx.buf)?;
        }
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::fs::fat32::prelude::*;

    #[test]
    fn test_fat32_injector_hierarchy_flow() {
        const SIZE_MB: u64 = 32;
        const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
        let meta = Fat32Meta::new(SIZE_BYTES, Some("TESTFS")).unwrap();

        let mut buf = vec![0u8; SIZE_BYTES as usize];

        let mut io = MemRimIO::new(&mut buf);
        let mut allocator = Fat32Allocator::new(&meta);

        let mut injector = Fat32Injector::new(&mut io, &mut allocator, &meta);

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
        injector.flush().unwrap();

        let mut parser_back = Fat32Resolver::new(&mut io, &meta);
        let mut parsed_tree = parser_back.parse_tree("/*").expect("parse_tree failed");

        tree.sort_children_recursively();
        parsed_tree.sort_children_recursively();

        println!("{tree}");
        println!("{parsed_tree}");
        assert!(tree.structural_eq(&parsed_tree), "Tree structure mismatch");
    }
}
