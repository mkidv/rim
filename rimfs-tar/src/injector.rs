// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::{String, ToString};

use crate::meta::TarMeta;
use crate::types::*;
use rimfs_core::errors::FsInjectorResult;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::attr::FileAttributes;
use rimio::{RimIO, RimRead};

/// Serializes and writes `FsNode` trees and files into a TAR archive stream.
pub struct TarInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a TarMeta,
    current_offset: u64,
    path_stack: alloc::vec::Vec<String>,
}

impl<'a, IO: RimIO + ?Sized> TarInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> FsInjectorResult<Self> {
        Ok(Self {
            io,
            _meta: meta,
            current_offset: 0,
            path_stack: alloc::vec::Vec::new(),
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

    #[allow(clippy::too_many_arguments)]
    pub fn write_header(
        &mut self,
        name: &str,
        size: u64,
        mode: u32,
        uid: u32,
        gid: u32,
        mtime: u64,
        typeflag: u8,
        linkname: &str,
    ) -> FsInjectorResult {
        let name_bytes = name.as_bytes();
        let link_bytes = linkname.as_bytes();

        // Check if name can be split into USTAR prefix (<= 155) + name (<= 100)
        let mut ustar_prefix: Option<(&str, &str)> = None;
        if name_bytes.len() > 100 {
            for (idx, &b) in name_bytes.iter().enumerate().rev() {
                if b == b'/' {
                    let prefix = &name[..idx];
                    let subname = &name[idx + 1..];
                    if !prefix.is_empty()
                        && prefix.len() <= 155
                        && !subname.is_empty()
                        && subname.len() <= 100
                    {
                        ustar_prefix = Some((prefix, subname));
                        break;
                    }
                }
            }
        }

        // If name > 100 and cannot be split via USTAR prefix, emit a GNU LongLink entry
        if name_bytes.len() > 100 && ustar_prefix.is_none() {
            let long_name_len = name_bytes.len() + 1;
            let mut long_hdr = [0u8; TAR_BLOCK_SIZE];
            long_hdr[..13].copy_from_slice(b"././@LongLink");
            format_octal(&mut long_hdr[100..108], 0o644);
            format_octal(&mut long_hdr[108..116], 0);
            format_octal(&mut long_hdr[116..124], 0);
            format_octal(&mut long_hdr[124..136], long_name_len as u64);
            format_octal(&mut long_hdr[136..148], 0);
            long_hdr[156] = GNULONGNAME;
            long_hdr[257..263].copy_from_slice(USTAR_MAGIC);
            long_hdr[263..265].copy_from_slice(USTAR_VERSION);
            let chk = calculate_checksum(&long_hdr);
            format_octal(&mut long_hdr[148..156], chk as u64);

            self.io.write_at(self.current_offset, &long_hdr)?;
            self.current_offset += TAR_BLOCK_SIZE as u64;

            let padded_len = (long_name_len + TAR_BLOCK_SIZE - 1) & !(TAR_BLOCK_SIZE - 1);
            let mut payload = alloc::vec![0u8; padded_len];
            payload[..name_bytes.len()].copy_from_slice(name_bytes);
            payload[name_bytes.len()] = 0;
            self.io.write_at(self.current_offset, &payload)?;
            self.current_offset += padded_len as u64;
        }

        // If linkname > 100, emit a GNU LongLink entry for the link target
        if link_bytes.len() > 100 {
            let long_link_len = link_bytes.len() + 1;
            let mut long_hdr = [0u8; TAR_BLOCK_SIZE];
            long_hdr[..13].copy_from_slice(b"././@LongLink");
            format_octal(&mut long_hdr[100..108], 0o644);
            format_octal(&mut long_hdr[108..116], 0);
            format_octal(&mut long_hdr[116..124], 0);
            format_octal(&mut long_hdr[124..136], long_link_len as u64);
            format_octal(&mut long_hdr[136..148], 0);
            long_hdr[156] = GNULONGLINK_TARGET;
            long_hdr[257..263].copy_from_slice(USTAR_MAGIC);
            long_hdr[263..265].copy_from_slice(USTAR_VERSION);
            let chk = calculate_checksum(&long_hdr);
            format_octal(&mut long_hdr[148..156], chk as u64);

            self.io.write_at(self.current_offset, &long_hdr)?;
            self.current_offset += TAR_BLOCK_SIZE as u64;

            let padded_len = (long_link_len + TAR_BLOCK_SIZE - 1) & !(TAR_BLOCK_SIZE - 1);
            let mut payload = alloc::vec![0u8; padded_len];
            payload[..link_bytes.len()].copy_from_slice(link_bytes);
            payload[link_bytes.len()] = 0;
            self.io.write_at(self.current_offset, &payload)?;
            self.current_offset += padded_len as u64;
        }

        let mut header = [0u8; TAR_BLOCK_SIZE];

        if let Some((prefix, subname)) = ustar_prefix {
            let sub_bytes = subname.as_bytes();
            header[..sub_bytes.len()].copy_from_slice(sub_bytes);
            let pref_bytes = prefix.as_bytes();
            header[345..345 + pref_bytes.len()].copy_from_slice(pref_bytes);
        } else {
            let name_len = name_bytes.len().min(100);
            header[..name_len].copy_from_slice(&name_bytes[..name_len]);
        }

        format_octal(&mut header[100..108], mode as u64);
        format_octal(&mut header[108..116], uid as u64);
        format_octal(&mut header[116..124], gid as u64);
        format_octal(&mut header[124..136], size);
        format_octal(&mut header[136..148], mtime);
        header[156] = typeflag;

        let link_len = link_bytes.len().min(100);
        header[157..157 + link_len].copy_from_slice(&link_bytes[..link_len]);

        header[257..263].copy_from_slice(USTAR_MAGIC);
        header[263..265].copy_from_slice(USTAR_VERSION);

        let chksum = calculate_checksum(&header);
        format_octal(&mut header[148..156], chksum as u64);

        self.io.write_at(self.current_offset, &header)?;
        self.current_offset += TAR_BLOCK_SIZE as u64;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> FsTreeInjector<TarHandle> for TarInjector<'a, IO> {
    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let mut dir_name = self.entry_path(name);
        if !dir_name.ends_with('/') {
            dir_name.push('/');
        }
        let mtime = attr
            .modified
            .map(|t| t.unix_timestamp() as u64)
            .unwrap_or(0);
        let mode = attr.mode.unwrap_or(0o755);
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        self.write_header(&dir_name, 0, mode, uid, gid, mtime, DIRTYPE, "")?;
        self.path_stack
            .push(normalize_fs_path(&dir_name).to_string());
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
        let mtime = attr
            .modified
            .map(|t| t.unix_timestamp() as u64)
            .unwrap_or(0);
        let mode = attr.mode.unwrap_or(0o644);
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        self.write_header(&file_name, size, mode, uid, gid, mtime, REGTYPE, "")?;

        // Stream file payload
        let mut buf = [0u8; 4096];
        let mut remaining = size;
        let mut src_off = 0;

        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            source.read_at(src_off, &mut buf[..to_read])?;
            self.io.write_at(self.current_offset, &buf[..to_read])?;
            self.current_offset += to_read as u64;
            src_off += to_read as u64;
            remaining -= to_read as u64;
        }

        // Pad to 512-byte boundary
        let pad = (TAR_BLOCK_SIZE - (size as usize % TAR_BLOCK_SIZE)) % TAR_BLOCK_SIZE;
        if pad > 0 {
            let zeros = [0u8; TAR_BLOCK_SIZE];
            self.io.write_at(self.current_offset, &zeros[..pad])?;
            self.current_offset += pad as u64;
        }

        Ok(())
    }

    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let link_name = self.entry_path(name);
        let mtime = attr
            .modified
            .map(|t| t.unix_timestamp() as u64)
            .unwrap_or(0);
        let mode = attr.mode.unwrap_or(0o777);
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        self.write_header(&link_name, 0, mode, uid, gid, mtime, SYMTYPE, target)
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
        // Write two 512-byte zero blocks to finish TAR
        let trailer = [0u8; 1024];
        self.io.write_at(self.current_offset, &trailer)?;
        self.current_offset += 1024;
        self.io.flush()?;
        Ok(())
    }
}
