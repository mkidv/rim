// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

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
}

impl<'a, IO: RimIO + ?Sized> ZipInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> FsInjectorResult<Self> {
        Ok(Self {
            io,
            meta,
            current_offset: 0,
            entries: Vec::new(),
            path_stack: Vec::new(),
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
        let mut header = [0u8; LOCAL_FILE_HEADER_FIXED_SIZE];
        header[0..4].copy_from_slice(&LOCAL_FILE_HEADER_SIG.to_le_bytes());

        let version_needed = if comp_size >= 0xFFFF_FFFF || uncomp_size >= 0xFFFF_FFFF {
            VERSION_NEEDED_ZIP64
        } else {
            VERSION_NEEDED_DEFAULT
        };
        header[4..6].copy_from_slice(&version_needed.to_le_bytes());
        header[6..8].copy_from_slice(&FLAG_UTF8_FILENAME.to_le_bytes());
        header[8..10].copy_from_slice(&compression_method.to_le_bytes());
        header[10..12].copy_from_slice(&time_dos.to_le_bytes());
        header[12..14].copy_from_slice(&date_dos.to_le_bytes());
        header[14..18].copy_from_slice(&crc32.to_le_bytes());

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
        header[18..22].copy_from_slice(&comp_32.to_le_bytes());
        header[22..26].copy_from_slice(&uncomp_32.to_le_bytes());

        let name_bytes = name.as_bytes();
        header[26..28].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        header[28..30].copy_from_slice(&(extra.len() as u16).to_le_bytes());

        self.io.write_at(lfh_offset, &header)?;
        self.io
            .write_at(lfh_offset + LOCAL_FILE_HEADER_FIXED_SIZE as u64, name_bytes)?;
        if !extra.is_empty() {
            self.io.write_at(
                lfh_offset + LOCAL_FILE_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64,
                extra,
            )?;
        }

        self.current_offset +=
            LOCAL_FILE_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64 + extra.len() as u64;
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
                // Read source into buffer for deflate encoding
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

        // Write all Central Directory entries
        for entry in &self.entries {
            let cd_offset = self.current_offset;
            let mut cdh = [0u8; CENTRAL_DIR_HEADER_FIXED_SIZE];
            cdh[0..4].copy_from_slice(&CENTRAL_DIR_HEADER_SIG.to_le_bytes());
            cdh[4..6].copy_from_slice(&VERSION_MADE_BY_UNIX.to_le_bytes());

            let needs_zip64 = entry.compressed_size >= 0xFFFF_FFFF
                || entry.uncompressed_size >= 0xFFFF_FFFF
                || entry.local_header_offset >= 0xFFFF_FFFF;

            let version_needed = if needs_zip64 {
                VERSION_NEEDED_ZIP64
            } else {
                VERSION_NEEDED_DEFAULT
            };
            cdh[6..8].copy_from_slice(&version_needed.to_le_bytes());
            cdh[8..10].copy_from_slice(&FLAG_UTF8_FILENAME.to_le_bytes());
            cdh[10..12].copy_from_slice(&entry.compression_method.to_le_bytes());
            cdh[12..14].copy_from_slice(&entry.mtime_dos.to_le_bytes());
            cdh[14..16].copy_from_slice(&entry.mdate_dos.to_le_bytes());
            cdh[16..20].copy_from_slice(&entry.crc32.to_le_bytes());

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
            cdh[20..24].copy_from_slice(&comp_32.to_le_bytes());
            cdh[24..28].copy_from_slice(&uncomp_32.to_le_bytes());

            let name_bytes = entry.name.as_bytes();
            cdh[28..30].copy_from_slice(&(name_bytes.len() as u16).to_le_bytes());

            let mut extra = Vec::new();
            if let Some(dt) = entry.timestamp {
                extra.extend_from_slice(&encode_extended_timestamp_extra(dt, false));
            }
            if let (Some(uid), Some(gid)) = (entry.uid, entry.gid) {
                extra.extend_from_slice(&encode_unix_uid_gid_extra(uid, gid));
            }
            if needs_zip64 {
                extra.extend_from_slice(&EXTRA_ZIP64_ID.to_le_bytes());
                let zip64_len: u16 = 24; // uncompressed (8) + compressed (8) + offset (8)
                extra.extend_from_slice(&zip64_len.to_le_bytes());
                extra.extend_from_slice(&entry.uncompressed_size.to_le_bytes());
                extra.extend_from_slice(&entry.compressed_size.to_le_bytes());
                extra.extend_from_slice(&entry.local_header_offset.to_le_bytes());
            }

            cdh[30..32].copy_from_slice(&(extra.len() as u16).to_le_bytes());
            // comment_len (32..34) = 0
            // disk_start (34..36) = 0
            // internal_attr (36..38) = 0
            cdh[38..42].copy_from_slice(&entry.external_attributes.to_le_bytes());

            let lfh_32 = if entry.local_header_offset >= 0xFFFF_FFFF {
                0xFFFF_FFFF
            } else {
                entry.local_header_offset as u32
            };
            cdh[42..46].copy_from_slice(&lfh_32.to_le_bytes());

            self.io.write_at(cd_offset, &cdh)?;
            self.io
                .write_at(cd_offset + CENTRAL_DIR_HEADER_FIXED_SIZE as u64, name_bytes)?;
            if !extra.is_empty() {
                self.io.write_at(
                    cd_offset + CENTRAL_DIR_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64,
                    &extra,
                )?;
            }

            self.current_offset +=
                CENTRAL_DIR_HEADER_FIXED_SIZE as u64 + name_bytes.len() as u64 + extra.len() as u64;
        }

        let cd_size = self.current_offset - cd_start_offset;
        let num_entries = self.entries.len();

        let needs_zip64_eocd =
            num_entries >= 0xFFFF || cd_size >= 0xFFFF_FFFF || cd_start_offset >= 0xFFFF_FFFF;

        if needs_zip64_eocd {
            let zip64_eocd_offset = self.current_offset;
            let mut eocd64 = [0u8; ZIP64_EOCD_FIXED_SIZE];
            eocd64[0..4].copy_from_slice(&ZIP64_END_OF_CENTRAL_DIR_SIG.to_le_bytes());
            let eocd64_size: u64 = 44; // size of remaining record
            eocd64[4..12].copy_from_slice(&eocd64_size.to_le_bytes());
            eocd64[12..14].copy_from_slice(&VERSION_MADE_BY_UNIX.to_le_bytes());
            eocd64[14..16].copy_from_slice(&VERSION_NEEDED_ZIP64.to_le_bytes());
            // disk_num (16..20) = 0, cd_disk (20..24) = 0
            eocd64[24..32].copy_from_slice(&(num_entries as u64).to_le_bytes());
            eocd64[32..40].copy_from_slice(&(num_entries as u64).to_le_bytes());
            eocd64[40..48].copy_from_slice(&cd_size.to_le_bytes());
            eocd64[48..56].copy_from_slice(&cd_start_offset.to_le_bytes());

            self.io.write_at(zip64_eocd_offset, &eocd64)?;
            self.current_offset += ZIP64_EOCD_FIXED_SIZE as u64;

            // ZIP64 Locator
            let locator_offset = self.current_offset;
            let mut locator = [0u8; ZIP64_LOCATOR_FIXED_SIZE];
            locator[0..4].copy_from_slice(&ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG.to_le_bytes());
            // cd_disk (4..8) = 0
            locator[8..16].copy_from_slice(&zip64_eocd_offset.to_le_bytes());
            locator[16..20].copy_from_slice(&1u32.to_le_bytes()); // total disks = 1

            self.io.write_at(locator_offset, &locator)?;
            self.current_offset += ZIP64_LOCATOR_FIXED_SIZE as u64;
        }

        // Standard EOCD Record
        let eocd_offset = self.current_offset;
        let mut eocd = [0u8; END_OF_CENTRAL_DIR_FIXED_SIZE];
        eocd[0..4].copy_from_slice(&END_OF_CENTRAL_DIR_SIG.to_le_bytes());
        // disk_num (4..6) = 0, cd_disk (6..8) = 0
        let entries_16 = if num_entries >= 0xFFFF {
            0xFFFF
        } else {
            num_entries as u16
        };
        eocd[8..10].copy_from_slice(&entries_16.to_le_bytes());
        eocd[10..12].copy_from_slice(&entries_16.to_le_bytes());

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
        eocd[12..16].copy_from_slice(&cd_size_32.to_le_bytes());
        eocd[16..20].copy_from_slice(&cd_offset_32.to_le_bytes());
        // comment_len (20..22) = 0

        self.io.write_at(eocd_offset, &eocd)?;
        self.current_offset += END_OF_CENTRAL_DIR_FIXED_SIZE as u64;

        self.io.flush()?;
        Ok(())
    }
}
