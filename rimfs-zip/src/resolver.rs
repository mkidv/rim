// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::meta::ZipMeta;
use crate::types::*;
use rimfs_core::errors::{FsResolverError, FsResolverResult};
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::{FsTreeResolver, attr::FileAttributes, attr::NodeKind};
use rimio::RimRead;
use rimio::prelude::*;

/// Filesystem resolver for ZIP archives and streams.
///
/// Parses the Central Directory into memory for fast $O(1)$ path and file resolution.
#[inline]
fn is_all_zeros(buf: &[u8]) -> bool {
    let (prefix, words, suffix) = unsafe { buf.align_to::<u64>() };
    for &b in prefix {
        if b != 0 {
            return false;
        }
    }
    for &w in words {
        if w != 0 {
            return false;
        }
    }
    for &b in suffix {
        if b != 0 {
            return false;
        }
    }
    true
}

pub struct ZipResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    _meta: &'a ZipMeta,
    entries: BTreeMap<String, ZipEntry>,
    dir_children: BTreeMap<String, Vec<String>>,
}

impl<'a, IO: RimRead + ?Sized> ZipResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> Self {
        let mut resolver = Self {
            io,
            _meta: meta,
            entries: BTreeMap::new(),
            dir_children: BTreeMap::new(),
        };
        let _ = resolver.load_central_directory();
        resolver
    }

    /// Locates and parses the End of Central Directory (EOCD) and Central Directory table.
    fn load_central_directory(&mut self) -> FsResolverResult<()> {
        let total_len = self.io.total_size().unwrap_or(0);
        if total_len < END_OF_CENTRAL_DIR_FIXED_SIZE as u64 {
            return Err(FsResolverError::Invalid("ZIP archive is too small"));
        }

        // Search backwards for EOCD signature across the storage stream
        let mut eocd_pos = None;
        let chunk_size = 262144usize;
        let mut scan_end = total_len;
        let mut chunk = vec![0u8; chunk_size];
        let eocd_sig_bytes = END_OF_CENTRAL_DIR_SIG.to_le_bytes();

        while scan_end >= END_OF_CENTRAL_DIR_FIXED_SIZE as u64 {
            let scan_start = scan_end.saturating_sub(chunk_size as u64);
            let cur_chunk_len = (scan_end - scan_start) as usize;
            self.io.read_at(scan_start, &mut chunk[..cur_chunk_len])?;

            if is_all_zeros(&chunk[..cur_chunk_len]) {
                scan_end = scan_start;
                continue;
            }

            if let Some(pos) = chunk[..cur_chunk_len]
                .windows(4)
                .rposition(|w| w == eocd_sig_bytes)
            {
                eocd_pos = Some(scan_start + pos as u64);
                break;
            }

            if scan_start == 0 {
                break;
            }

            scan_end = scan_start + (END_OF_CENTRAL_DIR_FIXED_SIZE as u64 - 1);
        }

        let eocd_offset = eocd_pos.ok_or(FsResolverError::Invalid(
            "End of Central Directory record not found",
        ))?;

        let mut eocd_buf = [0u8; END_OF_CENTRAL_DIR_FIXED_SIZE];
        self.io.read_at(eocd_offset, &mut eocd_buf)?;

        let mut total_entries = u16::from_le_bytes([eocd_buf[10], eocd_buf[11]]) as u64;
        let mut cd_size =
            u32::from_le_bytes([eocd_buf[12], eocd_buf[13], eocd_buf[14], eocd_buf[15]]) as u64;
        let mut cd_offset =
            u32::from_le_bytes([eocd_buf[16], eocd_buf[17], eocd_buf[18], eocd_buf[19]]) as u64;

        // Check for ZIP64 EOCD Locator right before standard EOCD
        if (total_entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF)
            && eocd_offset >= ZIP64_LOCATOR_FIXED_SIZE as u64
        {
            let locator_offset = eocd_offset - ZIP64_LOCATOR_FIXED_SIZE as u64;
            let mut locator_buf = [0u8; ZIP64_LOCATOR_FIXED_SIZE];
            if self.io.read_at(locator_offset, &mut locator_buf).is_ok() {
                let sig = u32::from_le_bytes([
                    locator_buf[0],
                    locator_buf[1],
                    locator_buf[2],
                    locator_buf[3],
                ]);
                if sig == ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG {
                    let eocd64_offset = u64::from_le_bytes([
                        locator_buf[8],
                        locator_buf[9],
                        locator_buf[10],
                        locator_buf[11],
                        locator_buf[12],
                        locator_buf[13],
                        locator_buf[14],
                        locator_buf[15],
                    ]);
                    let mut eocd64_buf = [0u8; ZIP64_EOCD_FIXED_SIZE];
                    if self.io.read_at(eocd64_offset, &mut eocd64_buf).is_ok() {
                        let sig64 = u32::from_le_bytes([
                            eocd64_buf[0],
                            eocd64_buf[1],
                            eocd64_buf[2],
                            eocd64_buf[3],
                        ]);
                        if sig64 == ZIP64_END_OF_CENTRAL_DIR_SIG {
                            total_entries = u64::from_le_bytes([
                                eocd64_buf[32],
                                eocd64_buf[33],
                                eocd64_buf[34],
                                eocd64_buf[35],
                                eocd64_buf[36],
                                eocd64_buf[37],
                                eocd64_buf[38],
                                eocd64_buf[39],
                            ]);
                            cd_size = u64::from_le_bytes([
                                eocd64_buf[40],
                                eocd64_buf[41],
                                eocd64_buf[42],
                                eocd64_buf[43],
                                eocd64_buf[44],
                                eocd64_buf[45],
                                eocd64_buf[46],
                                eocd64_buf[47],
                            ]);
                            cd_offset = u64::from_le_bytes([
                                eocd64_buf[48],
                                eocd64_buf[49],
                                eocd64_buf[50],
                                eocd64_buf[51],
                                eocd64_buf[52],
                                eocd64_buf[53],
                                eocd64_buf[54],
                                eocd64_buf[55],
                            ]);
                        }
                    }
                }
            }
        }
        let cd_len = checked_region_len(
            cd_offset,
            cd_size,
            eocd_offset,
            "Central Directory exceeds archive bounds",
        )?;
        let mut cd_buf = vec![0u8; cd_len];
        self.io.read_at(cd_offset, &mut cd_buf)?;

        let mut cd_pos = 0usize;
        for _ in 0..total_entries {
            let Some(cdh_end) = cd_pos.checked_add(CENTRAL_DIR_HEADER_FIXED_SIZE) else {
                break;
            };
            if cdh_end > cd_buf.len() {
                break;
            }
            let cdh_buf = &cd_buf[cd_pos..cdh_end];

            let sig = u32::from_le_bytes([cdh_buf[0], cdh_buf[1], cdh_buf[2], cdh_buf[3]]);
            if sig != CENTRAL_DIR_HEADER_SIG {
                break;
            }

            let compression_method = u16::from_le_bytes([cdh_buf[10], cdh_buf[11]]);
            let time_dos = u16::from_le_bytes([cdh_buf[12], cdh_buf[13]]);
            let date_dos = u16::from_le_bytes([cdh_buf[14], cdh_buf[15]]);
            let crc32 = u32::from_le_bytes([cdh_buf[16], cdh_buf[17], cdh_buf[18], cdh_buf[19]]);
            let comp_32 =
                u32::from_le_bytes([cdh_buf[20], cdh_buf[21], cdh_buf[22], cdh_buf[23]]) as u64;
            let uncomp_32 =
                u32::from_le_bytes([cdh_buf[24], cdh_buf[25], cdh_buf[26], cdh_buf[27]]) as u64;
            let name_len = u16::from_le_bytes([cdh_buf[28], cdh_buf[29]]) as usize;
            let extra_len = u16::from_le_bytes([cdh_buf[30], cdh_buf[31]]) as usize;
            let comment_len = u16::from_le_bytes([cdh_buf[32], cdh_buf[33]]) as usize;
            let external_attr =
                u32::from_le_bytes([cdh_buf[38], cdh_buf[39], cdh_buf[40], cdh_buf[41]]);
            let lfh_32 =
                u32::from_le_bytes([cdh_buf[42], cdh_buf[43], cdh_buf[44], cdh_buf[45]]) as u64;

            let name_start = cdh_end;
            let Some(name_end) = name_start.checked_add(name_len) else {
                break;
            };
            let Some(extra_end) = name_end.checked_add(extra_len) else {
                break;
            };
            if extra_end > cd_buf.len() {
                break;
            }

            let name_buf = &cd_buf[name_start..name_end];
            let raw_name = core::str::from_utf8(name_buf).unwrap_or("");
            let name_str = normalize_fs_path(raw_name).to_string();
            let extra_buf = &cd_buf[name_end..extra_end];

            let is_dir = raw_name.ends_with('/')
                || (external_attr & 0x10) != 0
                || ((external_attr >> 16) & 0o170000 == 0o040000);
            let is_symlink = (external_attr >> 16) & 0o170000 == 0o120000;
            let unix_mode = if (external_attr >> 16) > 0 {
                Some(external_attr >> 16)
            } else {
                None
            };

            let mut zip_entry = ZipEntry {
                name: name_str.clone(),
                compression_method,
                mtime_dos: time_dos,
                mdate_dos: date_dos,
                crc32,
                compressed_size: comp_32,
                uncompressed_size: uncomp_32,
                local_header_offset: lfh_32,
                external_attributes: external_attr,
                is_dir,
                is_symlink,
                unix_mode,
                uid: None,
                gid: None,
                timestamp: dos_to_datetime(time_dos, date_dos),
            };

            parse_extra_fields(extra_buf, &mut zip_entry, false);

            self.entries.insert(name_str, zip_entry);

            let Some(next_pos) = extra_end.checked_add(comment_len) else {
                break;
            };
            cd_pos = next_pos;
        }

        // Build O(1) parent -> children index
        let mut dir_children: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for key in self.entries.keys() {
            if key.is_empty() {
                continue;
            }
            let parts: Vec<&str> = key.split('/').collect();
            for i in 0..parts.len() {
                let parent = if i == 0 {
                    String::new()
                } else {
                    parts[..i].join("/")
                };
                let child = parts[i].to_string();
                let children = dir_children.entry(parent).or_default();
                if !children.contains(&child) {
                    children.push(child);
                }
            }
        }
        self.dir_children = dir_children;

        Ok(())
    }

    /// Resolves the actual data byte offset and length by inspecting the Local File Header.
    fn get_data_location(&mut self, entry: &ZipEntry) -> FsResolverResult<(u64, u64)> {
        let total_len = self.io.total_size().unwrap_or(0);
        checked_region_len(
            entry.local_header_offset,
            LOCAL_FILE_HEADER_FIXED_SIZE as u64,
            total_len,
            "Local File Header exceeds archive bounds",
        )?;

        let mut lfh_buf = [0u8; LOCAL_FILE_HEADER_FIXED_SIZE];
        self.io.read_at(entry.local_header_offset, &mut lfh_buf)?;

        let sig = u32::from_le_bytes([lfh_buf[0], lfh_buf[1], lfh_buf[2], lfh_buf[3]]);
        if sig != LOCAL_FILE_HEADER_SIG {
            return Err(FsResolverError::Invalid(
                "Invalid Local File Header signature",
            ));
        }

        let name_len = u16::from_le_bytes([lfh_buf[26], lfh_buf[27]]) as u64;
        let extra_len = u16::from_le_bytes([lfh_buf[28], lfh_buf[29]]) as u64;
        let data_offset = entry
            .local_header_offset
            .checked_add(LOCAL_FILE_HEADER_FIXED_SIZE as u64)
            .and_then(|v| v.checked_add(name_len))
            .and_then(|v| v.checked_add(extra_len))
            .ok_or(FsResolverError::Invalid(
                "Local File Header offset overflow",
            ))?;
        checked_region_len(
            data_offset,
            entry.compressed_size,
            total_len,
            "ZIP entry data exceeds archive bounds",
        )?;
        Ok((data_offset, entry.compressed_size))
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for ZipResolver<'a, IO> {
    fn exists(&mut self, path: &str) -> bool {
        let clean = normalize_fs_path(path);
        if clean.is_empty() {
            return true;
        }
        self.entries.contains_key(clean) || self.dir_children.contains_key(clean)
    }

    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let clean = normalize_fs_path(path);
        if let Some(children) = self.dir_children.get(clean) {
            return Ok(children.clone());
        }
        if clean.is_empty() || self.entries.contains_key(clean) {
            return Ok(Vec::new());
        }
        Err(FsResolverError::NotFound)
    }

    fn open_file<'c>(&'c mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'c>> {
        let clean = normalize_fs_path(path);
        let entry = self
            .entries
            .get(clean)
            .cloned()
            .ok_or(FsResolverError::NotFound)?;

        if entry.is_directory() {
            return Err(FsResolverError::Invalid("Path is a directory"));
        }

        let (data_offset, comp_size) = self.get_data_location(&entry)?;

        if entry.compression_method == METHOD_STORE {
            return Ok(Box::new(ExtentRimRead::from_contiguous(
                &mut *self.io,
                data_offset,
                entry.uncompressed_size,
            )));
        }

        #[cfg(feature = "deflate")]
        if entry.compression_method == METHOD_DEFLATE {
            let raw_len = usize::try_from(comp_size)
                .map_err(|_| FsResolverError::Invalid("Compressed entry is too large"))?;
            let mut raw_bytes = vec![0u8; raw_len];
            self.io.read_at(data_offset, &mut raw_bytes)?;
            let decompressed = miniz_oxide::inflate::decompress_to_vec(&raw_bytes)
                .map_err(|_| FsResolverError::Invalid("Deflate decompression failed"))?;
            return Ok(Box::new(VecRimIO::new(decompressed)));
        }

        let _ = comp_size;
        Err(FsResolverError::Invalid("Unsupported compression method"))
    }

    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        let clean = normalize_fs_path(path);
        let entry = self
            .entries
            .get(clean)
            .cloned()
            .ok_or(FsResolverError::NotFound)?;

        if !entry.is_symbolic_link() {
            return Err(FsResolverError::Invalid("Entry is not a symbolic link"));
        }

        let (data_offset, comp_size) = self.get_data_location(&entry)?;
        let raw_len = usize::try_from(comp_size)
            .map_err(|_| FsResolverError::Invalid("Symlink target is too large"))?;
        let mut raw_bytes = vec![0; raw_len];
        self.io.read_at(data_offset, &mut raw_bytes)?;

        let target_str = core::str::from_utf8(&raw_bytes)
            .map_err(|_| FsResolverError::Invalid("Invalid UTF-8 symlink target"))?;
        Ok(target_str.to_string())
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        let clean = normalize_fs_path(path);
        if clean.is_empty() {
            return Ok(FileAttributes::new_dir());
        }

        if let Some(entry) = self.entries.get(clean) {
            let kind = if entry.is_symbolic_link() {
                NodeKind::Symlink
            } else if entry.is_directory() {
                NodeKind::Directory
            } else {
                NodeKind::Regular
            };

            let mut attr = match kind {
                NodeKind::Directory => FileAttributes::new_dir(),
                NodeKind::Symlink => FileAttributes::new_symlink(),
                _ => FileAttributes::new_file(),
            };

            attr.mode = entry.unix_mode;
            attr.uid = entry.uid;
            attr.gid = entry.gid;
            attr.modified = entry.timestamp;

            return Ok(attr);
        }

        if self.dir_children.contains_key(clean) {
            return Ok(FileAttributes::new_dir());
        }

        Err(FsResolverError::NotFound)
    }
}

fn checked_region_len(
    offset: u64,
    len: u64,
    limit: u64,
    msg: &'static str,
) -> FsResolverResult<usize> {
    let end = offset
        .checked_add(len)
        .ok_or(FsResolverError::Invalid(msg))?;
    if end > limit {
        return Err(FsResolverError::Invalid(msg));
    }
    usize::try_from(len).map_err(|_| FsResolverError::Invalid(msg))
}
