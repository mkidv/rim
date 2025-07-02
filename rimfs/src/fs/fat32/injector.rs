// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::{BlockIO, BlockIOExt};

use crate::core::{injector::*, parser::*};

use crate::fs::fat32::{allocator::*, constant::*, meta::*, types::*};

pub struct Fat32Injector<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    allocator: &'a mut Fat32Allocator<'a>,
    meta: &'a Fat32Meta,
    stack: Vec<FsContext<Fat32Handle>>,
}

impl<'a, IO: BlockIO + ?Sized> Fat32Injector<'a, IO> {
    pub fn new(io: &'a mut IO, allocator: &'a mut Fat32Allocator<'a>, meta: &'a Fat32Meta) -> Self {
        Self {
            io,
            allocator,
            meta,
            stack: vec![],
        }
    }

    fn write_fat_chain(&mut self, cluster_chain: &[u32]) -> FsInjectorResult {
        let mut fat_entries = vec![0u8; cluster_chain.len() * FAT_ENTRY_SIZE];

        for (i, _cluster) in cluster_chain.iter().enumerate() {
            let next_cluster = if i + 1 < cluster_chain.len() {
                cluster_chain[i + 1]
            } else {
                FAT_EOC
            };
            let offset = i * FAT_ENTRY_SIZE;
            fat_entries[offset..offset + 4].copy_from_slice(&next_cluster.to_le_bytes());
        }

        for fat_index in 0..self.meta.num_fats {
            let offsets: Vec<u64> = cluster_chain
                .iter()
                .map(|&cluster| self.meta.fat_entry_offset(cluster, fat_index))
                .collect();

            self.io
                .write_multi_at(&offsets, FAT_ENTRY_SIZE, &fat_entries)?;
        }

        Ok(())
    }

    fn write_block(&mut self, block: u32, data: &[u8]) -> FsInjectorResult {
        let offset = self.meta.unit_offset(block);
        self.io
            .write_block_best_effort(offset, data, self.meta.unit_size())?;
        Ok(())
    }
}

impl<'a, IO: BlockIO + ?Sized> FsNodeInjector<Fat32Handle> for Fat32Injector<'a, IO> {
    fn set_root_context(&mut self, _: &FsNode) -> FsInjectorResult {
        let offset = self.meta.unit_offset(self.meta.root_unit());

        let mut buf = vec![0u8; self.meta.unit_size()];
        self.io.read_at(offset, &mut buf)?;

        if let Some(pos) = buf
            .chunks(32)
            .position(|entry| entry[0] == FAT_ENTRY_END_OF_DIR)
        {
            buf.truncate(pos * 32);
        }

        let handle = Fat32Handle::new(self.meta.root_unit());

        self.stack.push(FsContext::new(handle, buf));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let handle = self.allocator.allocate_unit()?;

        let mut buf = vec![];
        Fat32Entries::dot(handle.cluster_id).to_raw_buffer(&mut buf);
        Fat32Entries::dotdot(
            self.stack
                .last()
                .map(|ctx| ctx.handle.cluster_id)
                .unwrap_or(self.meta.root_unit()),
        )
        .to_raw_buffer(&mut buf);

        if let Some(parent) = self.stack.last_mut() {
            Fat32Entries::dir(name, handle.cluster_id, attr).to_raw_buffer(&mut parent.buf);
        }

        self.stack.push(FsContext::new(handle, buf));
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
            .allocate_chain(content.len().div_ceil(self.meta.unit_size()))?;

        self.write_fat_chain(&handle.cluster_chain)?;

        let cluster_size = self.meta.unit_size();

        if handle.cluster_chain.len() > 1 {
            let offsets: Vec<u64> = handle
                .cluster_chain
                .iter()
                .map(|&c| self.meta.unit_offset(c))
                .collect();

            let mut full_data = vec![0u8; offsets.len() * cluster_size];

            for (i, _) in handle.cluster_chain.iter().enumerate() {
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
            Fat32Entries::file(name, handle.cluster_id, content.len() as u32, attr)
                .to_raw_buffer(&mut ctx.buf);
        }
        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if let Some(ctx) = self.stack.pop() {
            eprintln!(
                "→ Flushing cluster {} ({} bytes)",
                ctx.handle.cluster_id,
                ctx.buf.len()
            );

            self.write_block(ctx.handle.cluster_id, &ctx.buf)?;
            self.write_fat_chain(&ctx.handle.cluster_chain)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        while let Some(ctx) = self.stack.pop() {
            eprintln!(
                "→ Flushing cluster {} ({} bytes)",
                ctx.handle.cluster_id,
                ctx.buf.len()
            );

            self.write_block(ctx.handle.cluster_id, &ctx.buf)?;
            self.write_fat_chain(&ctx.handle.cluster_chain)?;
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
        let meta = Fat32Meta::new(SIZE_BYTES, Some("TESTFS"));

        let mut buf = vec![0u8; SIZE_BYTES as usize];

        let mut io = MemBlockIO::new(&mut buf);
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

        let mut parser_back = Fat32Parser::new(&mut io, &meta);
        let mut parsed_tree = parser_back.parse_tree("/*").expect("parse_tree failed");

        tree.sort_children_recursively();
        parsed_tree.sort_children_recursively();

        println!("{tree}");
        println!("{parsed_tree}");
        assert_eq!(tree, parsed_tree);
    }
}
