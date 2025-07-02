// SPDX-License-Identifier: MIT
// rimgen/fs/ext4/builder.rs

use crate::{
    fs::ext4::allocator::Ext4MetadataAllocator,
    fs::ext4::{
        allocator::Ext4BlockAllocator, formater::Ext4Formatter, injector::Ext4Injector,
        params::Ext4Params,
    },
    core::{
        FsBuilder,
        allocator::{FsAllocator, FsMetadataAllocator},
        error::FsResult,
        formater::FsFormatter,
        inject_node_recursive,
        injector::FsInjector,
        io::FsBlockIO,
        parser::FsNode,
    },
};

pub struct Ext4Builder<'a, IO> {
    pub params: &'a Ext4Params,
    io: &'a mut IO,
    block_allocator: Ext4BlockAllocator<'a>,
    metadata_allocator: Ext4MetadataAllocator,
    formatter: Ext4Formatter,
}

impl<'a, IO> Ext4Builder<'a, IO>
where
    IO: FsBlockIO<u32>,
{
    pub fn new(io: &'a mut IO, params: &'a Ext4Params) -> Self {
        let block_allocator = Ext4BlockAllocator::new(params);
        let metadata_allocator = Ext4MetadataAllocator::new();
        let formatter = Ext4Formatter::new();

        Self {
            params,
            io,
            block_allocator,
            metadata_allocator,
            formatter,
        }
    }
}

impl<'a, IO> FsBuilder<u32, Ext4Params> for Ext4Builder<'a, IO>
where
    IO: FsBlockIO<u32>,
{
    fn format(&mut self) -> FsResult {
        self.formatter
            .format(self.io, &mut self.block_allocator, &self.params)?;
        Ok(())
    }

    fn inject_node(&mut self, node: &FsNode) -> FsResult {
        let mut injector = Ext4Injector::new(
            self.io,
            &mut self.block_allocator,
            &mut self.metadata_allocator,
            &self.params,
        );
        injector.begin()?;
        inject_node_recursive(node, &mut injector)?;
        injector.flush()?;
        Ok(())
    }

    fn finalize(&mut self) -> FsResult {
        #[cfg(debug_assertions)]
        {
            println!(
                "[rimgen] Used {} blocks, {} inodes.",
                self.block_allocator.used_blocks(),
                self.metadata_allocator.used_metadata()
            );
        }
        Ok(())
    }
}
