// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use rimio::{BlockIO, BlockIOExt};

pub use crate::core::parser::*;

use crate::core::utils::path_utils::*;
use crate::fs::exfat::{constant::*, meta::*, types::*};

pub struct ExFatParser<'a, IO: BlockIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ExFatMeta,
}

impl<'a, IO: BlockIO + ?Sized> ExFatParser<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ExFatMeta) -> Self {
        Self { io, meta }
    }
}

impl<'a, IO: BlockIO + ?Sized> FsParser for ExFatParser<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsParserResult<Vec<String>> {
        let (is_dir, cluster, _) = self.resolve_path(path)?;
        if !is_dir {
            return Err(FsParserError::Invalid("Not a directory"));
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
            return Err(FsParserError::Invalid("Not a file"));
        }

        let cluster_chain = read_cluster_chain(self.io, self.meta, cluster)?;
        let cluster_size = self.meta.cluster_size as usize;

        let mut content = Vec::with_capacity(size);

        if cluster_chain.len() > 1 {
            let offsets: Vec<u64> = cluster_chain
                .iter()
                .map(|&c| self.meta.unit_offset(c))
                .collect();

            let mut buf = vec![0u8; cluster_chain.len() * cluster_size];
            self.io.read_multi_at(&offsets, cluster_size, &mut buf)?;

            let size_to_read = size.min(buf.len());
            content.extend_from_slice(&buf[..size_to_read]);
        } else {
            let data = read_cluster_data(self.io, self.meta, cluster)?;
            content.extend_from_slice(&data[..size.min(data.len())]);
        }

        Ok(content)
    }

    fn read_attributes(&mut self, path: &str) -> FsParserResult<FileAttributes> {
        if path.is_empty() || path == "/" {
            return Ok(FileAttributes::new_dir());
        }

        let components = split_path(path);
        let mut current_cluster = self.meta.root_cluster;

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
    meta: &ExFatMeta,
    cluster: u32,
) -> FsParserResult<u32> {
    let entry_offset = meta.fat_entry_offset(cluster);
    let mut buf = [0u8; EXFAT_ENTRY_SIZE];
    io.read_at(entry_offset, &mut buf)?;
    let next_cluster = u32::from_le_bytes(buf);
    Ok(next_cluster)
}

fn read_cluster_chain<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
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
        if next >= EXFAT_EOC {
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
    meta: &ExFatMeta,
    cluster: u32,
) -> FsParserResult<Vec<u8>> {
    let cluster_offset = meta.unit_offset(cluster);
    let mut buf = vec![0u8; meta.unit_size()];
    io.read_block_best_effort(cluster_offset, &mut buf, meta.unit_size())?;
    Ok(buf)
}

fn read_dir_entries<IO: BlockIO + ?Sized>(
    io: &mut IO,
    meta: &ExFatMeta,
    start_cluster: u32,
) -> FsParserResult<Vec<ExFatEntries>> {
    let cluster_chain = read_cluster_chain(io, meta, start_cluster)?;
    let mut entries = vec![];
    let mut lfn_stack = vec![];
    let mut raw_primary: Option<[u8; 32]> = None;
    let mut raw_stream: Option<[u8; 32]> = None;

    for cluster in cluster_chain {
        let data = read_cluster_data(io, meta, cluster)?;
        for chunk in data.chunks_exact(32) {
            match chunk[0] {
                EXFAT_ENTRY_PRIMARY => {
                    // Flush previous entry if complete
                    if let (Some(primary), Some(stream)) = (raw_primary.take(), raw_stream.take()) {
                        let entry = ExFatEntries::from_raw(&lfn_stack, &primary, &stream)?;
                        entries.push(entry);
                    }
                    lfn_stack.clear();
                    raw_primary = Some(chunk.try_into().unwrap());
                    raw_stream = None;
                }
                EXFAT_ENTRY_STREAM => {
                    raw_stream = Some(chunk.try_into().unwrap());
                }
                EXFAT_ENTRY_NAME => {
                    lfn_stack.push(chunk.to_vec());
                }
                EXFAT_END_OF_DIR => {
                    if let (Some(primary), Some(stream)) = (raw_primary.take(), raw_stream.take()) {
                        let entry = ExFatEntries::from_raw(&lfn_stack, &primary, &stream)?;
                        entries.push(entry);
                    }
                    lfn_stack.clear();
                    raw_primary = None;
                    raw_stream = None;
                }
                _ => {
                    lfn_stack.clear();
                    raw_primary = None;
                    raw_stream = None;
                }
            }
        }
    }

    entries.sort_by_key(|e| e.name().unwrap_or_default().to_ascii_lowercase());
    Ok(entries)
}
