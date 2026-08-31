// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::String;

use crate::meta::TarMeta;
use crate::types::*;
use rimfs_core::errors::FsInjectorResult;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::{attr::FileAttributes, node::FsNode};
use rimio::{RimIO, RimRead};

/// Serializes and writes `FsNode` trees and files into a TAR archive stream.
pub struct TarInjector<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a TarMeta,
    current_offset: u64,
}

impl<'a, IO: RimIO + ?Sized> TarInjector<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> FsInjectorResult<Self> {
        Ok(Self {
            io,
            _meta: meta,
            current_offset: 0,
        })
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
        let mut header = [0u8; TAR_BLOCK_SIZE];

        let name_bytes = name.as_bytes();
        let name_len = name_bytes.len().min(100);
        header[..name_len].copy_from_slice(&name_bytes[..name_len]);

        format_octal(&mut header[100..108], mode as u64);
        format_octal(&mut header[108..116], uid as u64);
        format_octal(&mut header[116..124], gid as u64);
        format_octal(&mut header[124..136], size);
        format_octal(&mut header[136..148], mtime);
        header[156] = typeflag;

        let link_bytes = linkname.as_bytes();
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
        let mut dir_name = String::from(normalize_fs_path(name));
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
        self.write_header(&dir_name, 0, mode, uid, gid, mtime, DIRTYPE, "")
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let file_name = normalize_fs_path(name);
        let mtime = attr
            .modified
            .map(|t| t.unix_timestamp() as u64)
            .unwrap_or(0);
        let mode = attr.mode.unwrap_or(0o644);
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        self.write_header(file_name, size, mode, uid, gid, mtime, REGTYPE, "")?;

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
        let link_name = normalize_fs_path(name);
        let mtime = attr
            .modified
            .map(|t| t.unix_timestamp() as u64)
            .unwrap_or(0);
        let mode = attr.mode.unwrap_or(0o777);
        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        self.write_header(link_name, 0, mode, uid, gid, mtime, SYMTYPE, target)
    }

    fn set_root_context(&mut self, _node: &FsNode<'_>) -> FsInjectorResult {
        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
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
