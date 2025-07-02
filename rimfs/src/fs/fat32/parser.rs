// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};

use rimio::{BlockIO, BlockIOExt};

pub use crate::core::parser::*;

use crate::core::utils::path_utils::*;
use crate::fs::fat32::{attr::*, constant::*, meta::*, types::*};

pub struct Fat32Parser<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Fat32Meta,
}

impl<'a, IO: BlockIO + ?Sized> Fat32Parser<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Fat32Meta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: BlockIO + ?Sized> FsParser for Fat32Parser<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsParserResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_path(path)?;
        if !is_dir {
            return Err(FsParserError::Invalid("Root path is not a dir"));
        }

        let entries = read_dir_entries(self.io, self.meta, cluster)?;
        entries
            .into_iter()
            .map(|entry| entry.name())
            .collect::<Result<Vec<String>, _>>()
    }

    fn read_file(&mut self, path: &str) -> FsParserResult<Vec<u8>> {
        let (is_dir, cluster, size) = self.resolve_path(path)?;
        if is_dir {
            return Err(FsParserError::Invalid("Root path is not a dir"));
        }

        let cluster_chain = read_cluster_chain(self.io, self.meta, cluster)?;
        let cluster_size = self.meta.unit_size();

        // Use multi-read if big enough
        if cluster_chain.len() > 1 {
            let offsets: Vec<u64> = cluster_chain
                .iter()
                .map(|&c| self.meta.unit_offset(c))
                .collect();

            let mut buf = vec![0u8; cluster_chain.len() * cluster_size];

            self.io.read_multi_at(&offsets, cluster_size, &mut buf)?;

            // Truncate if needed
            Ok(buf[..size].to_vec())
        } else {
            // Fallback simple for 1 cluster file
            let data = read_cluster_data(self.io, self.meta, cluster)?;
            Ok(data[..(size).min(data.len())].to_vec())
        }
    }

    fn read_attributes(&mut self, path: &str) -> FsParserResult<FileAttributes> {
        if path.is_empty() || path == "/" {
            return Ok(FileAttributes::new_dir());
        }

        let components = split_path(path);
        let mut current_cluster = self.meta.root_unit();

        for (i, component) in components.iter().enumerate() {
            let entries = read_dir_entries(self.io, self.meta, current_cluster)?;
            let mut found = false;

            for entry in entries {
                if entry.name()?.eq_ignore_ascii_case(component) {
                    found = true;

                    if i == components.len() - 1 {
                        return Ok(entry.attr());
                    }

                    if !entry.is_dir() {
                        return Err(FsParserError::Invalid(
                            "Expected directory for intermediate component",
                        ));
                    }
                    current_cluster = entry.first_cluster();
                    break;
                }
            }

            if !found {
                return Err(FsParserError::Invalid("Path not found"));
            }
        }

        Err(FsParserError::Invalid("Invalid path"))
    }

    fn resolve_path(&mut self, path: &str) -> FsParserResult<(bool, u32, usize)> {
        if path.is_empty() || path == "/" {
            return Ok((true, self.meta.root_unit(), 0));
        }

        let components = split_path(path);
        let mut cluster = self.meta.root_unit();

        for (i, comp) in components.iter().enumerate() {
            let entries = read_dir_entries(self.io, self.meta, cluster)?;
            let mut found = false;

            for e in entries {
                if e.name_bytes_eq(comp) {
                    let is_last = i == components.len() - 1;
                    let is_dir = e.is_dir();
                    let next = e.first_cluster();
                    let size = e.size();

                    if !is_last && !is_dir {
                        return Err(FsParserError::Invalid("Expected directory"));
                    }

                    if is_last {
                        return Ok((is_dir, next, size));
                    }

                    cluster = next;
                    found = true;
                    break;
                }
            }

            if !found {
                return Err(FsParserError::Invalid("Path not found"));
            }
        }

        Err(FsParserError::Invalid("Invalid path"))
    }
}

pub fn read_fat_entry<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    cluster: u32,
) -> FsParserResult<u32> {
    let entry_offset = meta.fat_entry_offset(cluster, 0);
    let mut buf = [0u8; FAT_ENTRY_SIZE];
    io.read_at(entry_offset, &mut buf)?;
    let next_cluster = u32::from_le_bytes(buf) & 0x0FFFFFFF;
    Ok(next_cluster)
}

fn read_cluster_chain<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    start_cluster: u32,
) -> FsParserResult<Vec<u32>> {
    let mut chain = vec![];
    let mut current = start_cluster;

    loop {
        chain.push(current);
        if current < 2 || current >= meta.cluster_count + 2 {
            return Err(FsParserError::Other("Invalid cluster in FAT chain"));
        }

        let next = read_fat_entry(io, meta, current)?;
        if next >= FAT_EOC {
            break;
        }
        if chain.len() > meta.cluster_count as usize {
            return Err(FsParserError::Other("Invalid FAT chain"));
        }
        current = next;
    }

    Ok(chain)
}

fn read_cluster_data<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    cluster: u32,
) -> FsParserResult<Vec<u8>> {
    let cluster_offset = meta.unit_offset(cluster);
    let mut buf = vec![0u8; meta.unit_size()];
    io.read_block_best_effort(cluster_offset, &mut buf, meta.unit_size())?;
    Ok(buf)
}

fn read_dir_entries<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &Fat32Meta,
    start_cluster: u32,
) -> FsParserResult<Vec<Fat32Entries>> {
    let cluster_chain = read_cluster_chain(io, meta, start_cluster)?;
    let mut entries = vec![];
    let mut lfn_stack = vec![];

    for cluster in cluster_chain {
        let data = read_cluster_data(io, meta, cluster)?;
        for chunk in data.chunks_exact(32) {
            if chunk[0] == FAT_ENTRY_END_OF_DIR {
                lfn_stack.clear();
                break;
            }

            if chunk[0] == FAT_ENTRY_DELETED {
                lfn_stack.clear();
                continue;
            }

            if is_valid_lfn_entry(chunk) {
                lfn_stack.push(chunk.try_into().unwrap());
                continue;
            }

            if chunk[11] & Fat32Attributes::VOLUME_ID.bits() != 0 {
                lfn_stack.clear();
                continue;
            }

            let entry = Fat32Entries::from_raw(&lfn_stack, chunk)?;
            lfn_stack.clear();

            if &entry.entry.name[..2] == b". " || &entry.entry.name[..3] == b".. " {
                continue;
            }

            entries.push(entry);
        }
        lfn_stack.clear();
    }

    entries.sort_by_key(|e| e.name().unwrap_or_default().to_ascii_lowercase());
    Ok(entries)
}

fn is_valid_lfn_entry(entry: &[u8]) -> bool {
    entry.len() >= 32 && entry[11] == 0x0F
}
