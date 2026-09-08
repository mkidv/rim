// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::{
    string::{String, ToString},
    vec,
    vec::Vec,
};

use rimio::prelude::*;

use crate::core::{fat::*, injector::*, resolver::*};

use crate::upcase::UpcaseHandle;
use crate::{allocator::*, constant::*, meta::*, types::*, utils};

struct PendingDir {
    name: String,
    attr: FileAttributes,
    first_cluster: u32,
}

pub struct ExFatInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: ExFatAllocator<'a>,
    meta: &'a ExFatMeta,
    upcase: UpcaseHandle,
    stack: Vec<FsContext<ExFatHandle>>,
    pending_dirs: Vec<Option<PendingDir>>,
}

impl<'a, IO: RimIO + ?Sized> ExFatInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> FsInjectorResult<Self> {
        let upcase = UpcaseHandle::from_io(io, meta)?;
        let allocator = ExFatAllocator::from_io(io, meta).map_err(FsInjectorError::Allocator)?;

        Ok(Self {
            io,
            allocator,
            meta,
            upcase,
            stack: vec![],
            pending_dirs: vec![],
        })
    }

    fn ensure_chain_capacity(
        &mut self,
        handle: &mut ExFatHandle,
        needed: usize,
    ) -> FsInjectorResult {
        let current_len = handle.cluster_chain.total_units() as usize;
        if current_len >= needed {
            return Ok(());
        }
        let missing = needed - current_len;
        let extra: ExFatHandle = self.allocator.allocate(self.io, missing)?;
        handle.cluster_chain.extend(&extra.cluster_chain);
        Ok(())
    }

    fn write_chain_buffer(&mut self, handle: &ExFatHandle, buf: &[u8]) -> FsInjectorResult {
        let cs = self.meta.unit_size();

        // Zero-pad to avoid trailing junk entries from previous content
        let mut padded = buf.to_vec();
        let target_size = buf.len().div_ceil(cs) * cs;
        if padded.len() < target_size {
            padded.resize(target_size, 0);
        }

        if !handle.cluster_chain.is_contiguous() {
            let mut offsets = Vec::with_capacity(handle.cluster_chain.len());
            for run in handle.cluster_chain.iter() {
                for i in 0..run.length {
                    offsets.push(self.meta.unit_offset((run.start + i) as u32));
                }
            }

            self.io.write_multi_at(&offsets, cs, &padded)?;
        } else {
            let c = handle.cluster_chain.get_unit(0).unwrap() as u32;
            self.io
                .write_block_best_effort(self.meta.unit_offset(c), &padded, cs)?;
        }

        // Update FAT + bitmap for the entire chain
        let mut driver = FatDriver::new(self.meta);
        driver.write_run_list(self.io, &handle.cluster_chain)?;
        utils::write_bitmap(self.io, self.meta, &handle.cluster_chain)?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<ExFatHandle> for ExFatInjector<'a, IO> {
    fn set_root_context(&mut self, _: &FileAttributes) -> FsInjectorResult {
        let root = self.meta.root_unit();
        let mut driver = FatDriver::new(self.meta);
        let mut clusters = Vec::new();
        let mut cur = root;
        while cur >= 2 && cur <= self.meta.last_data_unit() && !self.meta.is_eoc(cur) {
            clusters.push(cur);
            if clusters.len() >= 100_000 {
                break;
            }
            match driver.get(self.io, cur) {
                Ok(next) => cur = next,
                Err(_) => break,
            }
        }
        let chain_clusters = if clusters.is_empty() {
            vec![root]
        } else {
            clusters
        };

        let cs = self.meta.unit_size();
        let mut buf = vec![0u8; chain_clusters.len() * cs];
        for (i, &c) in chain_clusters.iter().enumerate() {
            let offset = self.meta.unit_offset(c);
            self.io.read_at(offset, &mut buf[i * cs..(i + 1) * cs])?;
        }

        // Find the last non-empty entry to determine where to start adding new entries
        // Keep existing entries (Volume Label, Allocation Bitmap, Upcase Table, etc.)
        let eod_pos = buf
            .chunks(32)
            .position(|entry| entry[0] == EXFAT_EOD)
            .unwrap_or(buf.len() / 32);

        // Truncate to remove the end-of-directory marker and any trailing empty entries
        buf.truncate(eod_pos * 32);

        let handle = ExFatHandle::from_chain(RunList::from_units(&chain_clusters));
        self.stack.push(FsContext::new(handle, buf));
        self.pending_dirs.push(None);
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let handle: ExFatHandle = self.allocator.allocate_unit(self.io)?;

        // Open empty child context
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
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let entry = if size == 0 {
            ExFatEntries::file(name, 0, 0, attr, &self.upcase)
        } else {
            let cs = self.meta.unit_size();
            let need = size.div_ceil(cs as u64) as usize;
            let handle: ExFatHandle = self.allocator.allocate(self.io, need)?;

            use crate::core::utils::stream_copy::write_stream_to_run_list;
            write_stream_to_run_list(self.io, self.meta, source, &handle.cluster_chain, size)?;

            if handle.cluster_chain.is_contiguous() {
                ExFatEntries::file_contiguous(name, handle.cluster_id, size, attr, &self.upcase)
            } else {
                ExFatEntries::file(name, handle.cluster_id, size, attr, &self.upcase)
            }
        }
        .map_err(FsResolverError::Parsing)?;

        if let Some(ctx) = self.stack.last_mut() {
            entry.to_raw_buffer(&mut ctx.buf);
        }
        Ok(())
    }

    fn write_symlink(
        &mut self,
        _name: &str,
        _target: &str,
        _attr: &FileAttributes,
    ) -> FsInjectorResult {
        Err(FsInjectorError::Unsupported(
            "exFAT does not support symbolic links",
        ))
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
                // Round up to the next cluster
                let data_len = bytes_used.div_ceil(cluster_size as u64) * cluster_size as u64;

                ExFatEntries::dir_with_len(
                    &pd.name,
                    pd.first_cluster,
                    &pd.attr,
                    data_len,
                    &self.upcase,
                )
                .map_err(FsResolverError::Parsing)?
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
                // Round up to the next cluster
                let data_len = bytes_used.div_ceil(cluster_size as u64) * cluster_size as u64;

                ExFatEntries::dir_with_len(
                    &pd.name,
                    pd.first_cluster,
                    &pd.attr,
                    data_len,
                    &self.upcase,
                )
                .map_err(FsResolverError::Parsing)?
                .to_raw_buffer(&mut parent.buf);
            }
        }
        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use rimfs_core::testing::{assert_structural_tree_eq, nested_files_tree};

    #[test]
    fn test_exfat_injector() {
        const SIZE_MB: u64 = 32;
        const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
        let meta = ExFatMeta::new(SIZE_BYTES, Some("TESTFS")).unwrap();

        let mut buf = vec![0u8; SIZE_BYTES as usize];

        let mut io = MemRimIO::new(&mut buf);

        // Format the filesystem first
        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExFatInjector::new(&mut io, &meta).unwrap();

        let mut tree = nested_files_tree();

        injector.inject_tree(&mut tree).unwrap();

        let mut checker = ExFatChecker::new(&mut io, &meta);
        checker.fast_check().expect("check failed");

        let mut parser_back = ExFatResolver::new(&mut io, &meta);
        let mut parsed_tree = parser_back.resolve_tree("/*").expect("resolve_tree failed");

        assert_structural_tree_eq(&mut tree, &mut parsed_tree, "exFAT");
    }

    #[test]
    fn test_exfat_file_allocation_consistency() {
        use crate::constant::{EXFAT_ENTRY_STREAM, EXFAT_FIRST_CLUSTER};
        use crate::core::fat::*;
        use crate::types::ExFatStreamEntry;
        use zerocopy::FromBytes;

        const SIZE_MB: u64 = 5;
        const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
        let meta = ExFatMeta::new(SIZE_BYTES, Some("TESTALLOC")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        ExFatFormatter::new(&mut io, &meta)
            .format(false)
            .expect("Format failed");

        let mut injector = ExFatInjector::new(&mut io, &meta).expect("injector new failed");

        injector
            .set_root_context(&FileAttributes::new_dir())
            .expect("set_root_context failed");

        // 3 clusters roughly
        let file_size = 4096 * 3;
        let mut file_content = vec![0xAAu8; file_size];
        let mut file_io = MemRimIO::new(&mut file_content);
        injector
            .write_file(
                "test.bin",
                &mut file_io,
                file_size as u64,
                &FileAttributes::new_file(),
            )
            .expect("write_file failed");

        // flush to ensure root dir entry is written
        injector.flush().expect("flush failed");

        // Now manually verify FAT and Bitmap
        // 1. Find the file's first cluster from Root Directory
        let mut root_data = vec![0u8; meta.unit_size()];
        io.read_at(meta.unit_offset(meta.root_unit()), &mut root_data)
            .unwrap();

        let mut first_cluster = 0;
        let mut found = false;

        for chunk in root_data.chunks(32) {
            if chunk[0] == EXFAT_ENTRY_STREAM {
                let entry = ExFatStreamEntry::read_from_bytes(chunk).unwrap();
                first_cluster = entry.first_cluster;
                found = true;
                break;
            }
        }
        assert!(found, "File entry not found in root directory");
        assert!(
            first_cluster >= EXFAT_FIRST_CLUSTER,
            "Invalid first cluster"
        );

        // 2. Check FAT for this cluster
        let entries = FatDriver::new(&meta)
            .read_chain(&mut io, first_cluster)
            .expect("read_chain failed");
        assert!(!entries.is_empty(), "Chain should not be empty");

        let mut bitmap_data = vec![0u8; meta.bitmap_size_bytes as usize];
        io.read_at(meta.unit_offset(meta.bitmap_cluster), &mut bitmap_data)
            .expect("read bitmap failed");

        for &cluster in &entries {
            let idx = (cluster - EXFAT_FIRST_CLUSTER) as usize;
            let byte = idx / 8;
            let bit = idx % 8;
            let is_set = (bitmap_data[byte] & (1 << bit)) != 0;
            assert!(is_set, "Cluster {cluster} should be marked in bitmap");
        }
    }
}
