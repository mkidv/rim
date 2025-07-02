// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{BlockIO, BlockIOExt};
use zerocopy::FromBytes;

use crate::core::{injector::*, parser::*};

use crate::fs::exfat::{allocator::*, constant::*, meta::*, types::*, utils};

pub struct ExFatInjector<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    allocator: &'a mut ExFatAllocator<'a>,
    meta: &'a ExFatMeta,
    stack: Vec<FsContext<ExFatHandle>>,
}

impl<'a, IO: BlockIO + ?Sized> ExFatInjector<'a, IO> {
    pub fn new(io: &'a mut IO, allocator: &'a mut ExFatAllocator<'a>, meta: &'a ExFatMeta) -> Self {
        Self {
            io,
            allocator,
            meta,
            stack: vec![],
        }
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.meta.unit_offset(block);
        self.io
            .write_block_best_effort(offset, data, self.meta.cluster_size as usize)?;
        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsNodeInjector<ExFatHandle> for ExFatInjector<'a, IO> {
    fn set_root_context(&mut self, _: &FsNode) -> FsInjectorResult {
        let offset = self.meta.unit_offset(self.meta.root_unit());

        let mut buf = vec![0u8; self.meta.unit_size()];
        self.io.read_at(offset, &mut buf)?;

        // if let Some(pos) = buf
        //     .chunks(32)
        //     .position(|entry| ExFatEodEntry::read_from_bytes(entry).is_ok())
        // {
        //     buf.truncate((pos + 1) * 32);
        // }

        let handle = ExFatHandle::new(self.meta.root_unit());
        self.stack.push(FsContext::new(handle, buf));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let handle = self.allocator.allocate_unit()?;

        if let Some(parent) = self.stack.last_mut() {
            ExFatEntries::dir(name, handle.cluster_id, attr).to_raw_buffer(&mut parent.buf);
        }

        self.stack.push(FsContext::new(handle, vec![]));

        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        content: &[u8],
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let handle = self
            .allocator
            .allocate_chain(content.len().div_ceil(EXFAT_CLUSTER_SIZE as usize))?;

        utils::write_fat_chain(self.io, self.meta, &handle.cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &handle.cluster_chain)?;

        let cluster_size = self.meta.unit_size();

        if handle.cluster_chain.len() > 1 {
            let offsets: Vec<u64> = handle
                .cluster_chain
                .iter()
                .map(|&c| self.meta.unit_offset(c))
                .collect();

            let mut full_data = vec![0u8; offsets.len() * cluster_size];

            for (i, _cluster) in handle.cluster_chain.iter().enumerate() {
                let offset = i * cluster_size;
                let end = (offset + cluster_size).min(content.len());

                full_data[offset..offset + (end - offset)].copy_from_slice(&content[offset..end]);
            }

            self.io.write_multi_at(&offsets, cluster_size, &full_data)?;
        } else {
            let cluster = handle.cluster_chain[0];
            let end = cluster_size.min(content.len());

            self.write_block(cluster, &content[0..end])?;
        }

        if let Some(ctx) = self.stack.last_mut() {
            ExFatEntries::file(name, handle.cluster_id, content.len() as u32, attr)
                .to_raw_buffer(&mut ctx.buf);
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(mut ctx) = self.stack.pop() {
            // if !ctx
            //     .buf
            //     .chunks(32)
            //     .any(|entry| ExFatEodEntry::read_from_bytes(entry).is_ok())
            // {
            //     ExFatEodEntry::new().to_raw_buffer(&mut ctx.buf);
            // }
            self.write_block(ctx.handle.cluster_id, &ctx.buf)?;
            utils::write_fat_chain(self.io, self.meta, &ctx.handle.cluster_chain)?;
            utils::write_bitmap(self.io, self.meta, &ctx.handle.cluster_chain)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        while let Some(mut ctx) = self.stack.pop() {
            if !ctx
                .buf
                .chunks(32)
                .any(|entry| ExFatEodEntry::read_from_bytes(entry).is_ok())
            {
                ExFatEodEntry::new().to_raw_buffer(&mut ctx.buf);
            }

            self.write_block(ctx.handle.cluster_id, &ctx.buf)?;
            utils::write_fat_chain(self.io, self.meta, &ctx.handle.cluster_chain)?;
            utils::write_bitmap(self.io, self.meta, &ctx.handle.cluster_chain)?;
        }
        self.io.flush()?;
        Ok(())
    }
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

        let mut checker = ExFatChecker::new(&mut io, &meta);
        checker
            .check_bitmap_fat_consistency()
            .expect("check failed");

        let mut parser_back = ExFatParser::new(&mut io, &meta);
        let mut parsed_tree = parser_back.parse_tree("/*").expect("parse_tree failed");

        tree.sort_children_recursively();
        parsed_tree.sort_children_recursively();

        println!("{tree}");
        println!("{parsed_tree}");
        assert_eq!(tree, parsed_tree);
    }
}
