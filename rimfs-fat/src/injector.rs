// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::{String, ToString};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::core::utils::stream_copy::write_stream_to_run_list;
use crate::core::{injector::*, resolver::*};

use crate::core::fat::*;
use crate::{allocator::*, constant::*, meta::*, types::*};

pub struct FatInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    allocator: FatAllocator<'a>,
    meta: &'a FatMeta,
    // Stack of open directory contexts (top = current dir)
    stack: Vec<FsContext<FatHandle>>,
}

impl<'a, IO: RimIO + ?Sized> FatInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a FatMeta) -> FsInjectorResult<Self> {
        // We use the scanning allocator to ensure next_free_hint is correct
        let allocator = FatAllocator::from_io(io, meta)?;
        Ok(Self {
            io,
            allocator,
            meta,
            stack: vec![],
        })
    }

    fn ensure_chain_capacity(&mut self, handle: &mut FatHandle, needed: usize) -> FsInjectorResult {
        let current_len = handle.cluster_chain.total_units() as usize;
        if current_len >= needed {
            return Ok(());
        }
        let missing = needed - current_len;
        let extra: FatHandle = self.allocator.allocate(self.io, missing)?;
        handle.cluster_chain.extend(&extra.cluster_chain);
        Ok(())
    }

    fn write_chain_buffer(&mut self, handle: &FatHandle, buf: &[u8]) -> FsInjectorResult {
        let cs = self.meta.unit_size();

        if !handle.cluster_chain.is_contiguous() {
            // Write full chain using offsets
            let mut offsets = Vec::with_capacity(handle.cluster_chain.len());
            for run in handle.cluster_chain.iter() {
                for i in 0..run.length {
                    offsets.push(self.meta.unit_offset((run.start + i) as u32));
                }
            }

            let mut full = vec![0u8; offsets.len() * cs];
            full[..buf.len()].copy_from_slice(buf);
            self.io.write_multi_at(&offsets, cs, &full)?;
        } else {
            // Single-cluster or contiguous directory/data
            let c = handle.cluster_chain.get_unit(0).unwrap() as u32;
            self.io
                .write_block_best_effort(self.meta.unit_offset(c), buf, cs)?;
        }

        // Update FAT for the full chain
        let mut driver = FatDriver::new(self.meta);
        driver.write_run_list(self.io, &handle.cluster_chain)?;
        Ok(())
    }

    fn start_transaction(&mut self) -> FsInjectorResult {
        let mut buf = [0u8; 512];
        buf[0..5].copy_from_slice(b"DIRTY");
        self.io.write_at(
            FAT_TRANSACTION_SECTOR * self.meta.bytes_per_sector as u64,
            &buf,
        )?;
        self.io.flush()?;
        Ok(())
    }

    fn end_transaction(&mut self) -> FsInjectorResult {
        let buf = [0u8; 512]; // Zeroed = CLEAN
        self.io.write_at(
            FAT_TRANSACTION_SECTOR * self.meta.bytes_per_sector as u64,
            &buf,
        )?;
        self.io.flush()?;
        Ok(())
    }

    fn update_fat_checksum(&mut self) -> FsInjectorResult {
        if !self.meta.use_fat_integrity {
            return Ok(());
        }
        // Simplified: Read the whole FAT and calculate CRC32
        // In a real FS, we might do it incrementally.
        // For now, let's read the first FAT.
        let mut fat_buf =
            vec![0u8; (self.meta.fat_size_sectors * self.meta.bytes_per_sector as u32) as usize];
        self.io.read_at(self.meta.fat_offset_bytes, &mut fat_buf)?;

        let checksum = crate::core::utils::checksum_utils::crc32(&fat_buf);

        let free_clusters = self.allocator.free_count as u32;
        let next_free = self
            .allocator
            .next_free_hint
            .max(self.meta.first_data_unit());

        // Update FSINFO (Primary and Backup)
        for sector in [FAT_FSINFO_SECTOR, FAT_FSINFO_BACKUP_SECTOR] {
            let fsinfo_off = sector * self.meta.bytes_per_sector as u64;
            let mut fsinfo_buf = [0u8; 512];
            self.io.read_at(fsinfo_off, &mut fsinfo_buf)?;

            // Offset 476 is fat_checksum (RimFAT)
            fsinfo_buf[476..480].copy_from_slice(&checksum.to_le_bytes());
            // Offset 488 is free_cluster_count
            fsinfo_buf[488..492].copy_from_slice(&free_clusters.to_le_bytes());
            // Offset 492 is next_free_cluster
            fsinfo_buf[492..496].copy_from_slice(&next_free.to_le_bytes());

            self.io.write_at(fsinfo_off, &fsinfo_buf)?;
        }

        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<FatHandle> for FatInjector<'a, IO> {
    fn set_root_context(&mut self, _: &FsNode<'_>) -> FsInjectorResult {
        // Load root cluster’s existing entries, strip trailing EOD region
        let root = self.meta.root_unit();
        let offset = self.meta.unit_offset(root);

        let mut buf = vec![0u8; self.meta.root_dir_size_bytes()];
        self.io.read_at(offset, &mut buf)?;

        let eod_pos = buf
            .chunks(32)
            .position(|entry| entry[0] == FAT_EOD)
            .unwrap_or(buf.len() / 32);

        buf.truncate(eod_pos * 32);

        // Ensure the handle’s cluster_id equals the real root cluster (usually 2).
        let handle = FatHandle::new(self.meta.root_unit());

        self.stack.push(FsContext::new(handle, buf));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        // Allocate and IMMEDIATELY reserve the child dir’s first cluster in FAT (EOC) -> Done by allocator.
        let handle: FatHandle = self.allocator.allocate_unit(self.io)?;

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

        FatEntries::dot(handle.cluster_id, attr).to_raw_buffer(&mut child_buf);
        FatEntries::dotdot(parent_cluster, attr).to_raw_buffer(&mut child_buf);

        // Append the directory entry into the CURRENT parent now (size = 0).
        if let Some(parent) = self.stack.last_mut() {
            FatEntries::dir(name, handle.cluster_id, attr)
                .with_integrity_calculated(self.meta)
                .to_raw_buffer(&mut parent.buf)
        }

        // Push child context (we will write it at flush_current/flush).
        self.stack.push(FsContext::new(handle, child_buf));
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        // Allocate content chain and write file data first (best locality).
        let cs = self.meta.unit_size();
        let need = (size as usize).div_ceil(cs).max(1);
        let handle: FatHandle = self.allocator.allocate(self.io, need)?;

        // Stream content to disk
        if handle.cluster_chain.total_units() > 0 {
            write_stream_to_run_list(self.io, self.meta, source, &handle.cluster_chain, size)?;
        }

        // Append to parent
        if let Some(parent) = self.stack.last_mut() {
            let mut entries = FatEntries::file(name, handle.cluster_id, size as u32, attr);
            // Optimization: newly created files via injector are always contiguous (single allocation call)
            if self.meta.use_integrity {
                entries.contiguous_hint = true;
            }
            entries
                .with_integrity_calculated(self.meta)
                .to_raw_buffer(&mut parent.buf)
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
            "FAT does not support symbolic links",
        ))
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        // Write ONLY the current directory buffer; no parent linking here.
        if let Some(mut ctx) = self.stack.pop() {
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != FAT_EOD {
                FatEodEntry::new().to_raw_buffer(&mut ctx.buf);
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
        if self.meta.use_integrity {
            self.start_transaction()?;
        }
        // Drain remaining directory contexts; again, only data writes here.
        while let Some(mut ctx) = self.stack.pop() {
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != FAT_EOD {
                FatEodEntry::new().to_raw_buffer(&mut ctx.buf);
            }
            let cs = self.meta.unit_size();
            let used = ctx.buf.len();
            let need_clusters = used.div_ceil(cs).max(1);

            self.ensure_chain_capacity(&mut ctx.handle, need_clusters)?;
            self.write_chain_buffer(&ctx.handle, &ctx.buf)?;
        }

        if self.meta.use_integrity {
            self.update_fat_checksum()?;
            self.end_transaction()?;
        }

        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::injector::FsTreeInjector;
    use crate::prelude::*;
    use crate::resolver::FatResolver;
    use rimfs_core::testing::{assert_structural_tree_eq, nested_files_tree};

    fn test_injector_scenario(meta: FatMeta, name: &str) {
        let mut buf = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemRimIO::new(&mut buf);
        let mut injector = FatInjector::new(&mut io, &meta).unwrap();

        let mut tree = nested_files_tree();

        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut parser_back = FatResolver::new(&mut io, &meta);
        let mut parsed_tree = parser_back.resolve_tree("/*").expect("resolve_tree failed");

        assert_structural_tree_eq(&mut tree, &mut parsed_tree, name);
    }

    #[test]
    fn test_fat_variants() {
        const SIZE_MB: u64 = 32;
        const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

        // FAT32
        let meta32 = FatMeta::new_fat32(SIZE_BYTES, Some("FAT32")).unwrap();
        test_injector_scenario(meta32, "FAT32 Standard");

        // FAT16
        // 16MB volume is typically FAT16
        let meta16 = FatMeta::new_fat16(16 * 1024 * 1024, Some("FAT16")).unwrap();
        assert_eq!(meta16.bits, 16);
        test_injector_scenario(meta16, "FAT16");

        // FAT12
        // 1.44MB floppy size often uses FAT12
        let meta12 = FatMeta::new_fat12(1440 * 1024, Some("FAT12")).unwrap();
        assert_eq!(meta12.bits, 12);
        test_injector_scenario(meta12, "FAT12");

        // FAT8
        let meta8 = FatMeta::new_fat8(64 * 1024, Some("FAT8")).unwrap();
        assert_eq!(meta8.bits, 8);
        test_injector_scenario(meta8, "FAT8");

        // Exotic: Single FAT, huge clusters (128KB), 32 reserved sectors
        let meta_exotic = FatMeta::new_custom(
            32 * 1024 * 1024,
            Some("EXOTIC"),
            generate_volume_id_32(),
            1, // Single FAT
            FAT_SECTOR_SIZE,
            128 * 1024,
            32,
            0, // root_entry_count
            32,
        )
        .unwrap();
        assert_eq!(meta_exotic.num_fats, 1);
        assert_eq!(meta_exotic.bytes_per_cluster, 128 * 1024);
        test_injector_scenario(meta_exotic, "Exotic (1 FAT, 128KB clusters)");

        // Advanced Format (4KB Sectors)
        let meta_af = FatMeta::new_custom(
            32 * 1024 * 1024,
            Some("AF4K"),
            0x1234,
            2,
            4096, // 4KB sectors
            4096, // 1 cluster = 1 sector
            32,
            0,
            32,
        )
        .unwrap();
        test_injector_scenario(meta_af, "Advanced Format (4KB sectors)");

        // Multi-FAT (4 copies)
        let meta_multi = FatMeta::new_custom(
            16 * 1024 * 1024,
            Some("MULTIFAT"),
            0x5678,
            4, // 4 FAT copies!
            512,
            2048,
            32,
            0,
            32,
        )
        .unwrap();
        test_injector_scenario(meta_multi, "Multi-FAT (4 copies)");

        // Micro-Volume
        let meta_micro = FatMeta::new_fat12(64 * 1024, Some("MICRO")).unwrap();
        test_injector_scenario(meta_micro, "Micro-Volume (64KB)");

        // RIM-FAT (Integrity)
        let meta_rim = FatMeta::new_rimfat(1024 * 1024, Some("RIMFAT")).unwrap();
        assert!(meta_rim.use_integrity);

        // 1. Inject and Verify
        let mut buffer = vec![0u8; meta_rim.volume_size_bytes as usize];
        let mut io = rimio::prelude::MemRimIO::new(&mut buffer);
        let mut formatter = FatFormatter::new(&mut io, &meta_rim);
        formatter.format(true).unwrap();

        let node = FsNode::new_file("reli.txt", b"Reliable content".to_vec());

        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![node],
        };

        {
            let mut injector = FatInjector::new(&mut io, &meta_rim).unwrap();
            injector.inject_tree(&mut tree).unwrap();
        }

        // 2. Read with Resolver (Should work)
        {
            let mut resolver = FatResolver::new(&mut io, &meta_rim);
            let attr = resolver.read_attributes("/reli.txt").unwrap();
            assert!(attr.is_file());
            let content = resolver.read_file("/reli.txt").unwrap();
            assert_eq!(content, b"Reliable content");
        }

        // 3. Corrupt metadata byte of the file entry
        // Entry 0 = Volume Label, Entry 1 = reli.txt
        let root_off = meta_rim.unit_offset(meta_rim.root_unit());
        let mut entry_buffer = [0u8; 32];
        let file_entry_off = root_off + 32; // Offset for Entry 1
        io.read_at(file_entry_off, &mut entry_buffer).unwrap();

        // Corrupt first byte of name
        entry_buffer[0] ^= 0xFF;
        io.write_at(file_entry_off, &entry_buffer).unwrap();

        // 4. Read again (Should FAIL with CRC mismatch)
        {
            let mut resolver = FatResolver::new(&mut io, &meta_rim);
            let res = resolver.read_file("/reli.txt");
            assert!(res.is_err(), "Expected CRC error after corruption");
        }
    }
}
