// SPDX-License-Identifier: MIT

//! FAT directory tree and file injector.

#[cfg(all(not(feature = "std"), feature = "alloc", test))]
use alloc::string::ToString;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimfs_core::utils::checksum_utils::crc32_reader;
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
        let extra: FatHandle = self.allocator.allocate(self.io, missing as u64)?;
        handle.cluster_chain.extend(&extra.cluster_chain);
        Ok(())
    }

    fn write_chain_buffer(&mut self, handle: &FatHandle, buf: &[u8]) -> FsInjectorResult {
        let mut buf_offset = 0;
        for run in handle.cluster_chain.iter() {
            let run_offset = self.meta.unit_offset(run.start as u32);
            let run_bytes = (run.length * self.meta.unit_size()) as usize;
            if buf_offset < buf.len() {
                let chunk_len = (buf.len() - buf_offset).min(run_bytes);
                self.io
                    .write_at(run_offset, &buf[buf_offset..buf_offset + chunk_len])?;
                if chunk_len < run_bytes {
                    self.io.zero_at(
                        run_offset + chunk_len as u64,
                        (run_bytes - chunk_len) as u64,
                    )?;
                }
                buf_offset += chunk_len;
            } else {
                self.io.zero_at(run_offset, run_bytes as u64)?;
            }
        }

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
        let fat_size_bytes = self.meta.fat_size_sectors as u64 * self.meta.bytes_per_sector as u64;
        let checksum = crc32_reader(self.io, self.meta.fat_offset_bytes, fat_size_bytes)?;

        for sector in [FAT_FSINFO_SECTOR, FAT_FSINFO_BACKUP_SECTOR] {
            let fsinfo_off = sector * self.meta.bytes_per_sector as u64;
            let mut fsinfo: FatFsInfo = self.io.read_struct(fsinfo_off)?;

            // Keep the partial update tied to the canonical FSINFO layout.
            fsinfo.fat_checksum = checksum.into();
            self.io.write_struct(fsinfo_off, &fsinfo)?;
        }

        Ok(())
    }

    fn update_fsinfo(&mut self) -> FsInjectorResult {
        if self.meta.bits != 32 {
            return Ok(());
        }

        let free_clusters = self.allocator.free_count as u32;
        let next_free = self
            .allocator
            .next_free_hint
            .max(self.meta.first_data_unit());

        for sector in [FAT_FSINFO_SECTOR, FAT_FSINFO_BACKUP_SECTOR] {
            let fsinfo_off = sector * self.meta.bytes_per_sector as u64;
            let mut fsinfo: FatFsInfo = self.io.read_struct(fsinfo_off)?;

            // Offset 488 is free_cluster_count
            fsinfo.free_cluster_count = free_clusters.into();
            // Offset 492 is next_free_cluster
            fsinfo.next_free_cluster = next_free.into();

            self.io.write_struct(fsinfo_off, &fsinfo)?;
        }

        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<FatHandle> for FatInjector<'a, IO> {
    fn set_root_context(&mut self, _: &FileAttributes) -> FsInjectorResult {
        let root = self.meta.root_unit();
        let (chain_clusters, mut buf) = if self.meta.bits == 32 && root >= 2 {
            let mut cluster_list = Vec::new();
            let mut cursor = crate::core::cursor::ClusterCursor::new(self.meta, root);
            cursor
                .for_each_cluster(self.io, |_, cluster| {
                    cluster_list.push(cluster);
                    Ok(())
                })
                .map_err(crate::core::FsResolverError::from)?;
            let cs = self.meta.unit_size() as usize;
            let mut full_buf = vec![0u8; cluster_list.len() * cs];
            for (i, &c) in cluster_list.iter().enumerate() {
                let off = self.meta.unit_offset(c);
                self.io.read_at(off, &mut full_buf[i * cs..(i + 1) * cs])?;
            }
            (cluster_list, full_buf)
        } else {
            let offset = self.meta.unit_offset(root);
            let mut buf = vec![0u8; self.meta.root_dir_size_bytes()];
            self.io.read_at(offset, &mut buf)?;
            (vec![root], buf)
        };

        let eod_pos = buf
            .chunks(32)
            .position(|entry| entry[0] == FAT_EOD)
            .unwrap_or(buf.len() / 32);

        buf.truncate(eod_pos * 32);

        let handle = FatHandle::from_chain(RunList::from_units(&chain_clusters));

        self.stack.push(FsContext::new(handle, buf));
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        if name.encode_utf16().count() > 255 {
            return Err(FsInjectorError::Invalid("Name too long"));
        }
        let parent = self
            .stack
            .last()
            .ok_or(FsInjectorError::Invalid("Missing directory context"))?;
        FatEntries::dir_with_existing(name, 0, attr, &parent.buf)
            .map_err(crate::core::FsResolverError::from)?;
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

        let mut child_buf = Vec::with_capacity(self.meta.unit_size() as usize);

        FatEntries::dot(handle.cluster_id, attr).to_raw_buffer(&mut child_buf);
        FatEntries::dotdot(parent_cluster, attr).to_raw_buffer(&mut child_buf);

        if let Some(parent) = self.stack.last_mut() {
            FatEntries::dir_with_existing(name, handle.cluster_id, attr, &parent.buf)
                .map_err(crate::core::FsResolverError::from)?
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
        let size_u32 = u32::try_from(size)
            .map_err(|_| FsInjectorError::Invalid("File exceeds FAT size limit"))?;
        if name.encode_utf16().count() > 255 {
            return Err(FsInjectorError::Invalid("Name too long"));
        }
        let parent = self
            .stack
            .last()
            .ok_or(FsInjectorError::Invalid("Missing directory context"))?;
        FatEntries::file_with_existing(name, 0, size_u32, attr, &parent.buf)
            .map_err(crate::core::FsResolverError::from)?;
        // Prefer contiguous allocation for optimal sequential I/O and single-extent layout.
        let cs = self.meta.unit_size() as usize;
        let need = (size as usize).div_ceil(cs).max(1) as u64;
        let handle: FatHandle = self.allocator.allocate(self.io, need)?;

        // Stream content to disk
        if handle.cluster_chain.total_units() > 0 {
            write_stream_to_run_list(self.io, self.meta, source, &handle.cluster_chain, size)?;
        }

        if let Some(parent) = self.stack.last_mut() {
            let mut entries = FatEntries::file_with_existing(
                name,
                handle.cluster_id,
                size_u32,
                attr,
                &parent.buf,
            )
            .map_err(crate::core::FsResolverError::from)?;
            // Optimization: newly created files via injector are contiguous when single-run
            if self.meta.use_integrity && handle.cluster_chain.0.len() <= 1 {
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
        if let Some(mut ctx) = self.stack.pop() {
            if ctx.buf.len() >= 32 && ctx.buf[ctx.buf.len() - 32] != FAT_EOD {
                FatEodEntry::new().to_raw_buffer(&mut ctx.buf);
            }

            let cs = self.meta.unit_size() as usize;
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
        // Drain remaining directory contexts
        while !self.stack.is_empty() {
            self.flush_current()?;
        }

        if self.meta.bits == 32 {
            self.update_fsinfo()?;
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
    #[cfg(feature = "std")]
    use rimfs_core::StdResolver;
    use rimfs_core::testing::{assert_structural_tree_eq, nested_files_tree};
    #[cfg(feature = "std")]
    use std::io::Write;

    fn test_injector_scenario(meta: FatMeta, name: &str) {
        let mut buf = vec![0u8; meta.volume_size_bytes as usize];
        let mut io = MemRimIO::new(&mut buf);
        FatFormatter::new(&mut io, &meta).format(true).unwrap();
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

    #[test]
    #[cfg(feature = "std")]
    fn test_inject_tree_from_std_resolver_roundtrip() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024;

        let source_dir = tempfile::tempdir().unwrap();
        std::fs::write(source_dir.path().join("hello.txt"), b"hello from host\n").unwrap();
        std::fs::create_dir(source_dir.path().join("nested")).unwrap();

        let unicode_path = source_dir.path().join("nested").join("unicodé.txt");
        let mut unicode_file = std::fs::File::create(unicode_path).unwrap();
        unicode_file.write_all(b"bonjour\n").unwrap();

        let meta = FatMeta::new_fat32(SIZE_BYTES, Some("HOSTSRC")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();

        {
            let mut resolver = StdResolver::new();
            let mut injector = FatInjector::new(&mut io, &meta).unwrap();
            let path = source_dir.path().join("*");
            let counts = injector
                .inject_tree_from_resolver(&mut resolver, path.to_str().unwrap())
                .unwrap();

            assert_eq!(counts.dirs, 1);
            assert_eq!(counts.files, 2);
        }

        let mut resolver = FatResolver::new(&mut io, &meta);
        assert_eq!(
            resolver.read_file("hello.txt").unwrap(),
            b"hello from host\n"
        );
        assert_eq!(
            resolver.read_file("nested/unicodé.txt").unwrap(),
            b"bonjour\n"
        );
    }

    #[test]
    fn test_fat_sfn_collision_numeric_tails() {
        const SIZE_BYTES: u64 = 32 * 1024 * 1024;
        let meta = FatMeta::new_fat32(SIZE_BYTES, Some("SFNTEST")).unwrap();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();

        let mut injector = FatInjector::new(&mut io, &meta).unwrap();
        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: vec![
                FsNode::new_file("long_filename_alpha.txt", b"alpha content".to_vec()),
                FsNode::new_file("long_filename_beta.txt", b"beta content".to_vec()),
                FsNode::new_file("long_filename_gamma.txt", b"gamma content".to_vec()),
            ],
        };
        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();

        let mut resolver = FatResolver::new(&mut io, &meta);
        assert_eq!(
            resolver.read_file("long_filename_alpha.txt").unwrap(),
            b"alpha content"
        );
        assert_eq!(
            resolver.read_file("long_filename_beta.txt").unwrap(),
            b"beta content"
        );
        assert_eq!(
            resolver.read_file("long_filename_gamma.txt").unwrap(),
            b"gamma content"
        );

        let root_dir_entries = resolver.read_dir("/").unwrap();
        assert!(root_dir_entries.contains(&"long_filename_alpha.txt".to_string()));
        assert!(root_dir_entries.contains(&"long_filename_beta.txt".to_string()));
        assert!(root_dir_entries.contains(&"long_filename_gamma.txt".to_string()));
    }
    #[test]
    fn rimfat_checksum_writer_matches_fsinfo_reader() {
        let meta = FatMeta::new_rimfat(32 * 1024 * 1024, None).unwrap();
        let mut disk = vec![0; meta.volume_size_bytes as usize];
        let mut io = MemRimIO::new(&mut disk);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        FatInjector::new(&mut io, &meta)
            .unwrap()
            .update_fat_checksum()
            .unwrap();
        let mut fat = vec![0; meta.fat_size_sectors as usize * meta.bytes_per_sector as usize];
        io.read_at(meta.fat_offset_bytes, &mut fat).unwrap();
        let expected = crate::core::utils::checksum_utils::crc32(&fat);
        for sector in [FAT_FSINFO_SECTOR, FAT_FSINFO_BACKUP_SECTOR] {
            let info: FatFsInfo = io
                .read_struct(sector * meta.bytes_per_sector as u64)
                .unwrap();
            assert_eq!(info.fat_checksum.get(), expected);
            assert_eq!(info.struct_signature, FAT_FSINFO_STRUCT_SIGNATURE);
        }
        assert!(FatMeta::from_io(&mut io).is_ok());
        // The public reopening path must check the same field as the writer.
        fat[8] ^= 1;
        io.write_at(meta.fat_offset_bytes, &fat).unwrap();
        assert!(matches!(
            FatMeta::from_io(&mut io),
            Err(crate::core::FsError::Invalid(
                "RimFAT: Global FAT integrity failure"
            ))
        ));
    }
}
