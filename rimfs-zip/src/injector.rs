// SPDX-License-Identifier: MIT

//! ZIP archive stream injector supporting Deflate and Store compression.

use rimio::RimWriteStructExt;
use zerocopy::{FromBytes, IntoBytes};

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::formatter::clean_trailing_storage;
use crate::meta::ZipMeta;
use crate::types::*;
use crc32fast::Hasher;
use rimfs_core::errors::FsInjectorResult;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::attr::FileAttributes;
use rimio::{RimIO, RimRead};
use time::OffsetDateTime;

/// Serializes and writes `FsNode` trees, directories, files, and symlinks into a ZIP archive stream.
pub struct ZipInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a ZipMeta,
    current_offset: u64,
    entries: Vec<ZipEntry>,
    path_stack: Vec<String>,
    cleaned_tail_start: Option<u64>,
}

impl<'a, IO: RimIO + ?Sized> ZipInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> FsInjectorResult<Self> {
        Ok(Self {
            io,
            meta,
            current_offset: 0,
            entries: Vec::new(),
            path_stack: Vec::new(),
            cleaned_tail_start: None,
        })
    }

    fn entry_path(&self, name: &str) -> String {
        let name = normalize_fs_path(name);
        if let Some(parent) = self.path_stack.last()
            && !parent.is_empty()
        {
            return alloc::format!("{parent}/{name}");
        }
        name.to_string()
    }

    /// Helper to encode a Local File Header.
    #[allow(clippy::too_many_arguments)]
    fn write_local_header(
        &mut self,
        name: &str,
        compression_method: u16,
        time_dos: u16,
        date_dos: u16,
        crc32: u32,
        comp_size: u64,
        uncomp_size: u64,
        extra: &[u8],
    ) -> FsInjectorResult<u64> {
        let lfh_offset = self.current_offset;
        let mut header = ZipLocalFileHeader {
            signature: (LOCAL_FILE_HEADER_SIG).into(),
            ..Default::default()
        };

        let version_needed = if comp_size >= 0xFFFF_FFFF || uncomp_size >= 0xFFFF_FFFF {
            VERSION_NEEDED_ZIP64
        } else {
            VERSION_NEEDED_DEFAULT
        };
        header.version_needed = (version_needed).into();
        header.flags = (FLAG_UTF8_FILENAME).into();
        header.compression_method = (compression_method).into();
        header.mtime = (time_dos).into();
        header.mdate = (date_dos).into();
        header.crc32 = (crc32).into();

        let comp_32 = if comp_size >= 0xFFFF_FFFF {
            0xFFFF_FFFF
        } else {
            comp_size as u32
        };
        let uncomp_32 = if uncomp_size >= 0xFFFF_FFFF {
            0xFFFF_FFFF
        } else {
            uncomp_size as u32
        };
        header.compressed_size = (comp_32).into();
        header.uncompressed_size = (uncomp_32).into();

        let name_bytes = name.as_bytes();
        header.name_len = (name_bytes.len() as u16).into();

        let mut local_extra = extra.to_vec();
        if comp_size >= 0xFFFF_FFFF || uncomp_size >= 0xFFFF_FFFF {
            let zip64_len: u16 = 16;
            local_extra.extend_from_slice(
                crate::headers::ZipExtraFieldHeader {
                    id: EXTRA_ZIP64_ID.into(),
                    data_len: zip64_len.into(),
                }
                .as_bytes(),
            );
            local_extra.extend_from_slice(&uncomp_size.to_le_bytes());
            local_extra.extend_from_slice(&comp_size.to_le_bytes());
        }
        header.extra_len = (local_extra.len() as u16).into();

        self.io.write_struct(lfh_offset, &header)?;
        self.io
            .write_at(lfh_offset + LOCAL_FILE_HEADER_FIXED_SIZE as u64, name_bytes)?;
        if !local_extra.is_empty() {
            self.io.write_at(
                lfh_offset + LOCAL_FILE_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64,
                &local_extra,
            )?;
        }

        self.current_offset += LOCAL_FILE_HEADER_FIXED_SIZE as u64
            + name_bytes.len() as u64
            + local_extra.len() as u64;
        Ok(lfh_offset)
    }

    /// Rewrites the CRC32 and sizes in an already written Local File Header.
    fn patch_local_header(
        &mut self,
        lfh_offset: u64,
        crc32: u32,
        comp_size: u64,
        uncomp_size: u64,
    ) -> FsInjectorResult<()> {
        self.io.write_at(lfh_offset + 14, &crc32.to_le_bytes())?;
        let comp_32 = if comp_size >= 0xFFFF_FFFF {
            0xFFFF_FFFF
        } else {
            comp_size as u32
        };
        let uncomp_32 = if uncomp_size >= 0xFFFF_FFFF {
            0xFFFF_FFFF
        } else {
            uncomp_size as u32
        };
        self.io.write_at(lfh_offset + 18, &comp_32.to_le_bytes())?;
        self.io
            .write_at(lfh_offset + 22, &uncomp_32.to_le_bytes())?;

        if comp_size >= 0xFFFF_FFFF || uncomp_size >= 0xFFFF_FFFF {
            let mut lengths = [0u8; 4];
            self.io.read_at(lfh_offset + 26, &mut lengths)?;
            let name_len = u16::from_le_bytes([lengths[0], lengths[1]]) as u64;
            let extra_len = u16::from_le_bytes([lengths[2], lengths[3]]) as usize;
            let extra_offset = lfh_offset + LOCAL_FILE_HEADER_FIXED_SIZE as u64 + name_len;

            let mut extra_buf = alloc::vec![0u8; extra_len];
            self.io.read_at(extra_offset, &mut extra_buf)?;

            let mut offset = 0;
            while offset + 4 <= extra_buf.len() {
                let (header, _) =
                    crate::headers::ZipExtraFieldHeader::ref_from_prefix(&extra_buf[offset..])
                        .map_err(|_| rimio::RimIOError::Invalid("Truncated ZIP extra header"))?;
                let id = header.id.get();
                let sz = header.data_len.get() as usize;
                if id == EXTRA_ZIP64_ID && sz >= 16 && offset + 4 + sz <= extra_buf.len() {
                    let field_data_offset = extra_offset + (offset + 4) as u64;
                    self.io
                        .write_at(field_data_offset, &uncomp_size.to_le_bytes())?;
                    self.io
                        .write_at(field_data_offset + 8, &comp_size.to_le_bytes())?;
                    break;
                }
                offset += 4 + sz;
            }
        }
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<ZipHandle> for ZipInjector<'a, IO> {
    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let mut dir_name = self.entry_path(name);
        if !dir_name.ends_with('/') {
            dir_name.push('/');
        }

        let dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let (time_dos, date_dos) = datetime_to_dos(dt);
        let mode = attr.mode.unwrap_or(0o755) & 0o7777;
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);

        let mut extra = Vec::new();
        extra.extend_from_slice(&encode_extended_timestamp_extra(dt, true));
        extra.extend_from_slice(&encode_unix_uid_gid_extra(uid, gid));

        let lfh_offset =
            self.write_local_header(&dir_name, METHOD_STORE, time_dos, date_dos, 0, 0, 0, &extra)?;
        let stack_path = normalize_fs_path(&dir_name).to_string();

        self.entries.push(ZipEntry {
            name: dir_name,
            compression_method: METHOD_STORE,
            mtime_dos: time_dos,
            mdate_dos: date_dos,
            crc32: 0,
            compressed_size: 0,
            uncompressed_size: 0,
            local_header_offset: lfh_offset,
            external_attributes: ((mode | 0o040000) << 16) | 0x10, // Directory bit + unix mode
            is_dir: true,
            is_symlink: false,
            unix_mode: Some(mode | 0o040000),
            uid: Some(uid),
            gid: Some(gid),
            timestamp: Some(dt),
        });

        self.path_stack.push(stack_path);
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let file_name = self.entry_path(name);
        let dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let (time_dos, date_dos) = datetime_to_dos(dt);
        let mode = attr.mode.unwrap_or(0o644) & 0o7777;
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);

        let mut extra = Vec::new();
        extra.extend_from_slice(&encode_extended_timestamp_extra(dt, true));
        extra.extend_from_slice(&encode_unix_uid_gid_extra(uid, gid));

        let compression_method = self.meta.compression_method;

        #[cfg(feature = "deflate")]
        let use_deflate = compression_method == METHOD_DEFLATE;
        #[cfg(not(feature = "deflate"))]
        let use_deflate = false;
        #[cfg(not(feature = "deflate"))]
        let _ = compression_method;

        if use_deflate {
            #[cfg(feature = "deflate")]
            {
                let mut uncompressed = Vec::with_capacity(size.min(16 * 1024 * 1024) as usize);
                let mut buf = [0u8; 8192];
                let mut src_off = 0;
                let mut remaining = size;
                let mut hasher = Hasher::new();

                while remaining > 0 {
                    let to_read = remaining.min(buf.len() as u64) as usize;
                    source.read_at(src_off, &mut buf[..to_read])?;
                    hasher.update(&buf[..to_read]);
                    uncompressed.extend_from_slice(&buf[..to_read]);
                    src_off += to_read as u64;
                    remaining -= to_read as u64;
                }

                let crc32 = hasher.finalize();
                let compressed = miniz_oxide::deflate::compress_to_vec(&uncompressed, 6);
                let comp_size = compressed.len() as u64;

                let lfh_offset = self.write_local_header(
                    &file_name,
                    METHOD_DEFLATE,
                    time_dos,
                    date_dos,
                    crc32,
                    comp_size,
                    size,
                    &extra,
                )?;

                self.io.write_at(self.current_offset, &compressed)?;
                self.current_offset += comp_size;

                self.entries.push(ZipEntry {
                    name: file_name,
                    compression_method: METHOD_DEFLATE,
                    mtime_dos: time_dos,
                    mdate_dos: date_dos,
                    crc32,
                    compressed_size: comp_size,
                    uncompressed_size: size,
                    local_header_offset: lfh_offset,
                    external_attributes: (mode | 0o100000) << 16,
                    is_dir: false,
                    is_symlink: false,
                    unix_mode: Some(mode | 0o100000),
                    uid: Some(uid),
                    gid: Some(gid),
                    timestamp: Some(dt),
                });

                return Ok(());
            }
        }

        // Store (uncompressed) stream-first implementation
        let lfh_offset = self.write_local_header(
            &file_name,
            METHOD_STORE,
            time_dos,
            date_dos,
            0,
            size,
            size,
            &extra,
        )?;

        let mut buf = [0u8; 8192];
        let mut remaining = size;
        let mut src_off = 0;
        let mut hasher = Hasher::new();

        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            source.read_at(src_off, &mut buf[..to_read])?;
            hasher.update(&buf[..to_read]);
            self.io.write_at(self.current_offset, &buf[..to_read])?;
            self.current_offset += to_read as u64;
            src_off += to_read as u64;
            remaining -= to_read as u64;
        }

        let crc32 = hasher.finalize();
        self.patch_local_header(lfh_offset, crc32, size, size)?;

        self.entries.push(ZipEntry {
            name: file_name,
            compression_method: METHOD_STORE,
            mtime_dos: time_dos,
            mdate_dos: date_dos,
            crc32,
            compressed_size: size,
            uncompressed_size: size,
            local_header_offset: lfh_offset,
            external_attributes: (mode | 0o100000) << 16,
            is_dir: false,
            is_symlink: false,
            unix_mode: Some(mode | 0o100000),
            uid: Some(uid),
            gid: Some(gid),
            timestamp: Some(dt),
        });

        Ok(())
    }

    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let link_name = self.entry_path(name);
        let dt = attr.modified.unwrap_or(OffsetDateTime::UNIX_EPOCH);
        let (time_dos, date_dos) = datetime_to_dos(dt);
        let mode = attr.mode.unwrap_or(0o777) & 0o7777;
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);

        let mut extra = Vec::new();
        extra.extend_from_slice(&encode_extended_timestamp_extra(dt, true));
        extra.extend_from_slice(&encode_unix_uid_gid_extra(uid, gid));

        let target_bytes = target.as_bytes();
        let target_len = target_bytes.len() as u64;

        let mut hasher = Hasher::new();
        hasher.update(target_bytes);
        let crc32 = hasher.finalize();

        let lfh_offset = self.write_local_header(
            &link_name,
            METHOD_STORE,
            time_dos,
            date_dos,
            crc32,
            target_len,
            target_len,
            &extra,
        )?;

        self.io.write_at(self.current_offset, target_bytes)?;
        self.current_offset += target_len;

        self.entries.push(ZipEntry {
            name: link_name,
            compression_method: METHOD_STORE,
            mtime_dos: time_dos,
            mdate_dos: date_dos,
            crc32,
            compressed_size: target_len,
            uncompressed_size: target_len,
            local_header_offset: lfh_offset,
            external_attributes: ((mode | 0o120000) << 16) | 0x20, // Symlink type in unix mode
            is_dir: false,
            is_symlink: true,
            unix_mode: Some(mode | 0o120000),
            uid: Some(uid),
            gid: Some(gid),
            timestamp: Some(dt),
        });

        Ok(())
    }

    fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
        self.path_stack.clear();
        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        self.path_stack.pop();
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        let cd_start_offset = self.current_offset;
        let mut cursor = cd_start_offset;

        for entry in &self.entries {
            let cd_offset = cursor;
            let mut cdh = ZipCentralDirectoryHeader {
                signature: (CENTRAL_DIR_HEADER_SIG).into(),
                ..Default::default()
            };
            cdh.version_made_by = (VERSION_MADE_BY_UNIX).into();

            let needs_zip64 = entry.compressed_size >= 0xFFFF_FFFF
                || entry.uncompressed_size >= 0xFFFF_FFFF
                || entry.local_header_offset >= 0xFFFF_FFFF;

            let version_needed = if needs_zip64 {
                VERSION_NEEDED_ZIP64
            } else {
                VERSION_NEEDED_DEFAULT
            };
            cdh.version_needed = (version_needed).into();
            cdh.flags = (FLAG_UTF8_FILENAME).into();
            cdh.compression_method = (entry.compression_method).into();
            cdh.mtime = (entry.mtime_dos).into();
            cdh.mdate = (entry.mdate_dos).into();
            cdh.crc32 = (entry.crc32).into();

            let comp_32 = if entry.compressed_size >= 0xFFFF_FFFF {
                0xFFFF_FFFF
            } else {
                entry.compressed_size as u32
            };
            let uncomp_32 = if entry.uncompressed_size >= 0xFFFF_FFFF {
                0xFFFF_FFFF
            } else {
                entry.uncompressed_size as u32
            };
            cdh.compressed_size = (comp_32).into();
            cdh.uncompressed_size = (uncomp_32).into();

            let name_bytes = entry.name.as_bytes();
            cdh.name_len = (name_bytes.len() as u16).into();

            let mut extra = Vec::new();
            if let Some(dt) = entry.timestamp {
                extra.extend_from_slice(&encode_extended_timestamp_extra(dt, false));
            }
            if let (Some(uid), Some(gid)) = (entry.uid, entry.gid) {
                extra.extend_from_slice(&encode_unix_uid_gid_extra(uid, gid));
            }
            if needs_zip64 {
                let zip64_len: u16 = 8
                    * ((entry.uncompressed_size >= 0xFFFF_FFFF) as u16
                        + (entry.compressed_size >= 0xFFFF_FFFF) as u16
                        + (entry.local_header_offset >= 0xFFFF_FFFF) as u16);
                extra.extend_from_slice(
                    crate::headers::ZipExtraFieldHeader {
                        id: EXTRA_ZIP64_ID.into(),
                        data_len: zip64_len.into(),
                    }
                    .as_bytes(),
                );
                if entry.uncompressed_size >= 0xFFFF_FFFF {
                    extra.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
                }
                if entry.compressed_size >= 0xFFFF_FFFF {
                    extra.extend_from_slice(&entry.compressed_size.to_le_bytes());
                }
                if entry.local_header_offset >= 0xFFFF_FFFF {
                    extra.extend_from_slice(&entry.local_header_offset.to_le_bytes());
                }
            }

            cdh.extra_len = (extra.len() as u16).into();
            cdh.external_attributes = (entry.external_attributes).into();

            let lfh_32 = if entry.local_header_offset >= 0xFFFF_FFFF {
                0xFFFF_FFFF
            } else {
                entry.local_header_offset as u32
            };
            cdh.local_header_offset = (lfh_32).into();

            self.io.write_struct(cd_offset, &cdh)?;
            self.io
                .write_at(cd_offset + CENTRAL_DIR_HEADER_FIXED_SIZE as u64, name_bytes)?;
            if !extra.is_empty() {
                self.io.write_at(
                    cd_offset + CENTRAL_DIR_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64,
                    &extra,
                )?;
            }

            cursor +=
                CENTRAL_DIR_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64 + extra.len() as u64;
        }

        let cd_size = cursor - cd_start_offset;
        let num_entries = self.entries.len();

        let needs_zip64_eocd =
            num_entries >= 0xFFFF || cd_size >= 0xFFFF_FFFF || cd_start_offset >= 0xFFFF_FFFF;

        if needs_zip64_eocd {
            let zip64_eocd_offset = cursor;
            let mut eocd64 = Zip64Eocd {
                signature: (ZIP64_END_OF_CENTRAL_DIR_SIG).into(),
                ..Default::default()
            };
            let eocd64_size: u64 = 44; // size of remaining record
            eocd64.record_size = (eocd64_size).into();
            eocd64.version_made_by = (VERSION_MADE_BY_UNIX).into();
            eocd64.version_needed = (VERSION_NEEDED_ZIP64).into();
            eocd64.disk_entries = (num_entries as u64).into();
            eocd64.total_entries = (num_entries as u64).into();
            eocd64.directory_size = (cd_size).into();
            eocd64.directory_offset = (cd_start_offset).into();

            self.io.write_struct(zip64_eocd_offset, &eocd64)?;
            cursor += ZIP64_EOCD_FIXED_SIZE as u64;

            // ZIP64 Locator
            let locator_offset = cursor;
            let mut locator = Zip64Locator {
                signature: (ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG).into(),
                ..Default::default()
            };
            locator.eocd_offset = (zip64_eocd_offset).into();
            locator.total_disks = (1u32).into(); // total disks = 1

            self.io.write_struct(locator_offset, &locator)?;
            cursor += ZIP64_LOCATOR_FIXED_SIZE as u64;
        }

        let eocd_offset = cursor;
        let mut eocd = ZipEocd {
            signature: (END_OF_CENTRAL_DIR_SIG).into(),
            ..Default::default()
        };
        let entries_16 = if num_entries >= 0xFFFF {
            0xFFFF
        } else {
            num_entries as u16
        };
        eocd.disk_entries = (entries_16).into();
        eocd.total_entries = (entries_16).into();

        let cd_size_32 = if cd_size >= 0xFFFF_FFFF {
            0xFFFF_FFFF
        } else {
            cd_size as u32
        };
        let cd_offset_32 = if cd_start_offset >= 0xFFFF_FFFF {
            0xFFFF_FFFF
        } else {
            cd_start_offset as u32
        };
        eocd.directory_size = (cd_size_32).into();
        eocd.directory_offset = (cd_offset_32).into();

        // Clear stale trailing archive records before publishing this EOCD.
        let archive_end = eocd_offset
            .checked_add(END_OF_CENTRAL_DIR_FIXED_SIZE as u64)
            .ok_or(rimio::RimIOError::OutOfBounds)?;
        let storage_end = self.io.total_size()?;
        let clean_start = archive_end;
        let clean_end = self
            .cleaned_tail_start
            .unwrap_or(storage_end)
            .min(storage_end);
        if clean_start < clean_end {
            clean_trailing_storage(self.io, clean_start, clean_end)?;
        }
        self.cleaned_tail_start = Some(archive_end);
        self.io.write_struct(eocd_offset, &eocd)?;

        self.io.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::core::traits::FsTreeResolver;
    use alloc::vec;
    use rimio::RimWrite;
    #[test]
    fn zip64_offset_only_roundtrip() {
        let mut io = rimio::SparseRimIO::new((1u64 << 32) + 65536);
        let meta = ZipMeta::default();
        let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
        injector.current_offset = (1u64 << 32) + 512;
        let mut source = rimio::SliceRimIO::new(b"content");
        injector
            .write_file("probe", &mut source, 7, &FileAttributes::new_file())
            .unwrap();
        injector.flush().unwrap();
        let mut resolver = crate::ZipResolver::try_new(&mut io, &meta).unwrap();
        assert_eq!(resolver.read_file("probe").unwrap(), b"content");
    }
    #[test]
    fn stored_payload_checks_crc_including_zero_and_size() {
        let meta = ZipMeta::default();
        let mut storage = vec![0; 65536];
        let mut io = rimio::MemRimIO::new(&mut storage);
        let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
        injector
            .write_file(
                "probe",
                &mut rimio::SliceRimIO::new(b"content"),
                7,
                &FileAttributes::new_file(),
            )
            .unwrap();
        let central = injector.current_offset;
        injector.flush().unwrap();
        let mut good = vec![0; io.total_size().unwrap() as usize];
        io.read_at(0, &mut good).unwrap();
        for (offset, value) in [(central + 16, 0u32), (central + 24, 8u32)] {
            let mut storage = good.clone();
            let mut damaged = rimio::MemRimIO::new(&mut storage);
            damaged.write_at(offset, &value.to_le_bytes()).unwrap();
            let mut resolver = crate::ZipResolver::try_new(&mut damaged, &meta).unwrap();
            assert!(resolver.open_file("probe").is_err());
        }
        let mut damaged = rimio::MemRimIO::new(&mut good);
        damaged.write_at(central, &[0; 4]).unwrap();
        assert!(crate::ZipResolver::try_new(&mut damaged, &meta).is_err());
    }
    #[cfg(feature = "deflate")]
    #[test]
    fn deflate_budget_and_zero_crc_are_enforced() {
        let meta = ZipMeta {
            compression_method: METHOD_DEFLATE,
            ..ZipMeta::default()
        };
        let mut disk = vec![0; 65536];
        let mut io = rimio::MemRimIO::new(&mut disk);
        let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
        injector
            .write_file(
                "probe",
                &mut rimio::SliceRimIO::new(&[7; 4096]),
                4096,
                &FileAttributes::new_file(),
            )
            .unwrap();
        let central = injector.current_offset;
        injector.flush().unwrap();
        {
            let mut resolver = crate::ZipResolver::with_payload_budget(&mut io, &meta, 1024);
            assert!(resolver.open_file("probe").is_err());
        }
        {
            let mut resolver = crate::ZipResolver::try_new(&mut io, &meta).unwrap();
            assert_eq!(resolver.read_file("probe").unwrap(), [7; 4096]);
        }
        io.write_at(central + 16, &0u32.to_le_bytes()).unwrap();
        let mut resolver = crate::ZipResolver::try_new(&mut io, &meta).unwrap();
        assert!(resolver.open_file("probe").is_err());
    }
}
