// SPDX-License-Identifier: MIT

//! ZIP archive central directory parser and path resolver.

use rimio::RimReadStructExt;
use zerocopy::FromBytes;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::meta::ZipMeta;
use crate::types::*;
use rimfs_core::errors::{FsResolverError, FsResolverResult};
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::{FsTreeResolver, PathIndex, attr::FileAttributes, attr::NodeKind};
use rimio::RimRead;
use rimio::prelude::*;

#[inline]
fn is_all_zeros(buf: &[u8]) -> bool {
    buf.iter().all(|&b| b == 0)
}

pub struct ZipResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    _meta: &'a ZipMeta,
    index: PathIndex<ZipEntry>,
    status: FsResolverResult<()>,
    payload_budget: usize,
}

impl<'a, IO: RimRead + ?Sized> ZipResolver<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> Self {
        Self::with_payload_budget(io, meta, 1024 * 1024 * 1024)
    }

    /// Limit the decoded payload allocation. Compressed input uses a fixed 8 KiB scratch buffer; decoder state and metadata are separate.
    pub fn with_payload_budget(io: &'a mut IO, meta: &'a ZipMeta, payload_budget: usize) -> Self {
        let mut resolver = Self {
            io,
            _meta: meta,
            index: PathIndex::new(),
            status: Ok(()),
            payload_budget,
        };
        resolver.status = resolver.load_central_directory();
        resolver
    }

    /// Maximum decoded payload allocation; compressed input is read in bounded chunks.
    pub fn payload_budget(&self) -> usize {
        self.payload_budget
    }

    /// Try to construct a new ZipResolver, returning an error if central directory is invalid
    pub fn try_new(io: &'a mut IO, meta: &'a ZipMeta) -> FsResolverResult<Self> {
        let resolver = Self::new(io, meta);
        resolver.status?;
        Ok(resolver)
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

        let eocd_buf: ZipEocd = self.io.read_struct(eocd_offset)?;

        let mut total_entries = eocd_buf.total_entries.get() as u64;
        let mut cd_size = eocd_buf.directory_size.get() as u64;
        let mut cd_offset = eocd_buf.directory_offset.get() as u64;

        if (total_entries == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF)
            && eocd_offset >= ZIP64_LOCATOR_FIXED_SIZE as u64
        {
            let locator_offset = eocd_offset - ZIP64_LOCATOR_FIXED_SIZE as u64;
            if let Ok(locator_buf) = self.io.read_struct::<Zip64Locator>(locator_offset) {
                let sig = locator_buf.signature.get();
                if sig == ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG {
                    let eocd64_offset = locator_buf.eocd_offset.get();
                    if let Ok(eocd64_buf) = self.io.read_struct::<Zip64Eocd>(eocd64_offset) {
                        let sig64 = eocd64_buf.signature.get();
                        if sig64 == ZIP64_END_OF_CENTRAL_DIR_SIG {
                            total_entries = eocd64_buf.total_entries.get();
                            cd_size = eocd64_buf.directory_size.get();
                            cd_offset = eocd64_buf.directory_offset.get();
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
                return Err(FsResolverError::Invalid("Incomplete central directory"));
            };
            if cdh_end > cd_buf.len() {
                return Err(FsResolverError::Invalid("Incomplete central directory"));
            }
            let cdh_buf = ZipCentralDirectoryHeader::ref_from_bytes(&cd_buf[cd_pos..cdh_end])
                .map_err(|_| rimio::RimIOError::Invalid("Invalid ZIP central header"))?;

            let sig = cdh_buf.signature.get();
            if sig != CENTRAL_DIR_HEADER_SIG {
                return Err(FsResolverError::Invalid("Incomplete central directory"));
            }

            if cdh_buf.flags.get() & 0x41 != 0 {
                return Err(FsResolverError::Unsupported);
            }
            let compression_method = cdh_buf.compression_method.get();
            let time_dos = cdh_buf.mtime.get();
            let date_dos = cdh_buf.mdate.get();
            let crc32 = cdh_buf.crc32.get();
            let comp_32 = cdh_buf.compressed_size.get() as u64;
            let uncomp_32 = cdh_buf.uncompressed_size.get() as u64;
            let name_len = cdh_buf.name_len.get() as usize;
            let extra_len = cdh_buf.extra_len.get() as usize;
            let comment_len = cdh_buf.comment_len.get() as usize;
            let external_attr = cdh_buf.external_attributes.get();
            let lfh_32 = cdh_buf.local_header_offset.get() as u64;

            let name_start = cdh_end;
            let Some(name_end) = name_start.checked_add(name_len) else {
                return Err(FsResolverError::Invalid("Incomplete central directory"));
            };
            let Some(extra_end) = name_end.checked_add(extra_len) else {
                return Err(FsResolverError::Invalid("Incomplete central directory"));
            };
            if extra_end > cd_buf.len() {
                return Err(FsResolverError::Invalid("Incomplete central directory"));
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

            self.index.insert_with_kind(&name_str, zip_entry, is_dir);

            let Some(next_pos) = extra_end.checked_add(comment_len) else {
                return Err(FsResolverError::Invalid("Incomplete central directory"));
            };
            cd_pos = next_pos;
        }

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

        let lfh_buf: ZipLocalFileHeader = self.io.read_struct(entry.local_header_offset)?;

        let sig = lfh_buf.signature.get();
        if sig != LOCAL_FILE_HEADER_SIG {
            return Err(FsResolverError::Invalid(
                "Invalid Local File Header signature",
            ));
        }

        let name_len = lfh_buf.name_len.get() as u64;
        let extra_len = lfh_buf.extra_len.get() as u64;
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
        if self.status.is_err() {
            return false;
        }
        self.index.contains_path(path)
    }

    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        self.status?;
        let clean = normalize_fs_path(path);
        let trimmed = clean.trim_end_matches('/');
        if self
            .index
            .get(trimmed)
            .is_some_and(|entry| !entry.is_directory())
        {
            return Err(FsResolverError::Invalid("Path is not a directory"));
        }
        self.index
            .children(trimmed)
            .ok_or(FsResolverError::NotFound)
    }

    fn open_file<'c>(&'c mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'c>> {
        self.status?;
        let clean = normalize_fs_path(path);
        let entry = self
            .index
            .get(clean)
            .cloned()
            .ok_or(FsResolverError::NotFound)?;

        if entry.is_directory() {
            return Err(FsResolverError::Invalid("Path is a directory"));
        }

        let (data_offset, comp_size) = self.get_data_location(&entry)?;

        if entry.compression_method == METHOD_STORE {
            if comp_size != entry.uncompressed_size {
                return Err(FsResolverError::Invalid("Stored size mismatch"));
            }
            let mut scratch = [0u8; 8192];
            let mut hasher = crc32fast::Hasher::new();
            let mut done = 0u64;
            while done < comp_size {
                let n = (comp_size - done).min(scratch.len() as u64) as usize;
                self.io.read_at(data_offset + done, &mut scratch[..n])?;
                hasher.update(&scratch[..n]);
                done += n as u64;
            }
            if hasher.finalize() != entry.crc32 {
                return Err(FsResolverError::Invalid("CRC32 checksum mismatch"));
            }
            return Ok(Box::new(ExtentRimRead::from_contiguous(
                &mut *self.io,
                data_offset,
                entry.uncompressed_size,
            )));
        }

        #[cfg(feature = "deflate")]
        if entry.compression_method == METHOD_DEFLATE {
            let output_len = usize::try_from(entry.uncompressed_size)
                .map_err(|_| FsResolverError::Invalid("Uncompressed size too large"))?;
            if output_len > self.payload_budget {
                return Err(FsResolverError::Invalid(
                    "ZIP payload exceeds memory budget",
                ));
            }
            let decompressed = inflate_payload(self.io, data_offset, comp_size, output_len)?;

            if decompressed.len() as u64 != entry.uncompressed_size {
                return Err(FsResolverError::Invalid("Decompressed size mismatch"));
            }

            {
                let mut hasher = crc32fast::Hasher::new();
                hasher.update(&decompressed);
                if hasher.finalize() != entry.crc32 {
                    return Err(FsResolverError::Invalid("CRC32 checksum mismatch"));
                }
            }

            return Ok(Box::new(VecRimIO::new(decompressed)));
        }

        let _ = comp_size;
        Err(FsResolverError::Invalid("Unsupported compression method"))
    }

    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        self.status?;
        let clean = normalize_fs_path(path);
        let entry = self
            .index
            .get(clean)
            .cloned()
            .ok_or(FsResolverError::NotFound)?;

        if !entry.is_symbolic_link() {
            return Err(FsResolverError::Invalid("Entry is not a symbolic link"));
        }

        let mut reader = self.open_file(path)?;
        let size = reader.total_size()?;
        if size > 65536 {
            return Err(FsResolverError::Invalid("Symlink target exceeds limit"));
        }
        let mut target = vec![0; size as usize];
        reader.read_at(0, &mut target)?;
        String::from_utf8(target)
            .map_err(|_| FsResolverError::Invalid("Invalid UTF-8 symlink target"))
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        self.status?;
        let clean = normalize_fs_path(path);
        let trimmed = clean.trim_end_matches('/');
        if trimmed.is_empty() {
            return Ok(FileAttributes::new_dir());
        }

        if let Some(entry) = self.index.get(trimmed) {
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

            let dos_attr = (entry.external_attributes & 0xFF) as u8;
            attr.read_only = dos_attr & 0x01 != 0;
            attr.hidden = dos_attr & 0x02 != 0;
            attr.system = dos_attr & 0x04 != 0;
            attr.archive = dos_attr & 0x20 != 0;

            attr.mode = entry.unix_mode;
            attr.uid = entry.uid;
            attr.gid = entry.gid;
            attr.modified = entry.timestamp;

            return Ok(attr);
        }

        if self.index.is_dir(trimmed) {
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

/// Retain decoded bytes for random access without materializing compressed input.
#[cfg(feature = "deflate")]
fn inflate_payload<IO: RimRead + ?Sized>(
    io: &mut IO,
    offset: u64,
    compressed_len: u64,
    output_len: usize,
) -> FsResolverResult<Vec<u8>> {
    use miniz_oxide::inflate::{
        TINFLStatus,
        core::{DecompressorOxide, decompress, inflate_flags::*},
    };
    let mut state = DecompressorOxide::new();
    let mut output = vec![0; output_len];
    let mut input = [0u8; 8192];
    let (mut loaded, mut start, mut end, mut written) = (0u64, 0usize, 0usize, 0usize);
    loop {
        if start == end && loaded < compressed_len {
            end = (compressed_len - loaded).min(input.len() as u64) as usize;
            io.read_at(offset + loaded, &mut input[..end])?;
            loaded += end as u64;
            start = 0;
        }
        let flags = TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF
            | if loaded < compressed_len {
                TINFL_FLAG_HAS_MORE_INPUT
            } else {
                0
            };
        let (status, consumed, produced) =
            decompress(&mut state, &input[start..end], &mut output, written, flags);
        start += consumed;
        written += produced;
        match status {
            TINFLStatus::Done
                if written == output_len && start == end && loaded == compressed_len =>
            {
                return Ok(output);
            }
            TINFLStatus::NeedsMoreInput if consumed != 0 || produced != 0 => {}
            _ => {
                return Err(FsResolverError::Invalid(
                    "Deflate stream or declared size is invalid",
                ));
            }
        }
    }
}

#[cfg(all(test, feature = "deflate"))]
mod streaming_tests {
    use super::*;
    #[test]
    fn bounded_input_decodes_chunks_and_enforces_sizes() {
        let mut seed = 7u32;
        let random: Vec<u8> = (0..50000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        for data in [Vec::new(), vec![42; 50000], random] {
            let mut compressed = miniz_oxide::deflate::compress_to_vec(&data, 6);
            let size = compressed.len() as u64;
            let mut io = rimio::MemRimIO::new(&mut compressed);
            assert_eq!(inflate_payload(&mut io, 0, size, data.len()).unwrap(), data);
            assert!(inflate_payload(&mut io, 0, size, data.len() + 1).is_err());
            if !data.is_empty() {
                assert!(inflate_payload(&mut io, 0, size, data.len() - 1).is_err());
            }
            assert!(inflate_payload(&mut io, 0, size - 1, data.len()).is_err());
        }
    }
}
