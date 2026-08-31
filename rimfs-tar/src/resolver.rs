// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};

use crate::meta::TarMeta;
use crate::types::*;
use rimfs_core::errors::{FsResolverError, FsResolverResult};
use rimfs_core::normalize_fs_path;
use rimfs_core::resolver::{FsTreeResolver, attr::FileAttributes, attr::NodeKind};
use rimio::RimRead;
use rimio::prelude::*;

/// Filesystem resolver for TAR archives and streams.
///
/// Implements [`FsTreeResolver`] over any [`RimRead`] storage stream.
pub struct TarResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    _meta: &'a TarMeta,
}

impl<'a, IO: RimRead + ?Sized> TarResolver<'a, IO> {
    /// Creates a new `TarResolver` over an existing storage stream and metadata.
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> Self {
        Self { io, _meta: meta }
    }

    /// Read and parse the TAR header at the given byte offset.
    fn read_header_at(&mut self, offset: u64) -> FsResolverResult<Option<TarEntry<'static>>> {
        let mut header = [0u8; TAR_BLOCK_SIZE];
        if self.io.read_at(offset, &mut header).is_err() {
            return Ok(None);
        }
        if header.iter().all(|&b| b == 0) {
            return Ok(None);
        }

        let mut name_end = 0;
        while name_end < 100 && header[name_end] != 0 {
            name_end += 1;
        }
        let name_str = core::str::from_utf8(&header[..name_end]).unwrap_or("");
        let mode = parse_octal(&header[100..108]) as u32;
        let uid = parse_octal(&header[108..116]) as u32;
        let gid = parse_octal(&header[116..124]) as u32;
        let size = parse_octal(&header[124..136]);
        let mtime = parse_octal(&header[136..148]);
        let typeflag = header[156];

        let mut link_end = 0;
        while link_end < 100 && header[157 + link_end] != 0 {
            link_end += 1;
        }
        let link_str = core::str::from_utf8(&header[157..157 + link_end]).unwrap_or("");

        let data_offset = offset + TAR_BLOCK_SIZE as u64;
        let entry_name = normalize_fs_path(name_str);

        Ok(Some(TarEntry {
            name: entry_name.to_string(),
            mode,
            uid,
            gid,
            size,
            mtime,
            typeflag,
            link_name: link_str.to_string(),
            data_offset,
            data_slice: None,
        }))
    }

    /// Resolves an entry by searching TAR headers sequentially.
    pub fn resolve_entry(&mut self, path: &str) -> FsResolverResult<(TarEntry<'static>, u64)> {
        let norm_path = normalize_fs_path(path);
        let mut offset = 0u64;

        while let Some(entry) = self.read_header_at(offset)? {
            let entry_name = normalize_fs_path(&entry.name);
            if entry_name == norm_path {
                return Ok((entry, offset));
            }

            let padded_size = (entry.size as usize + TAR_BLOCK_SIZE - 1) & !(TAR_BLOCK_SIZE - 1);
            offset += (TAR_BLOCK_SIZE + padded_size) as u64;
        }

        Err(FsResolverError::NotFound)
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for TarResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let norm_path = normalize_fs_path(path);
        let mut entries = Vec::new();
        let mut offset = 0u64;

        while let Some(entry) = self.read_header_at(offset)? {
            let entry_name = normalize_fs_path(&entry.name);
            let (matches, remainder) = if norm_path.is_empty() {
                (true, entry_name)
            } else if let Some(stripped) = entry_name.strip_prefix(norm_path) {
                let rem = stripped.trim_start_matches('/');
                if stripped.starts_with('/') || stripped.is_empty() {
                    (true, rem)
                } else {
                    (false, "")
                }
            } else {
                (false, "")
            };

            if matches && !remainder.is_empty() {
                let first_comp = remainder.split('/').next().unwrap_or("");
                if !first_comp.is_empty() && !entries.contains(&first_comp.to_string()) {
                    entries.push(first_comp.to_string());
                }
            }

            let padded_size = (entry.size as usize + TAR_BLOCK_SIZE - 1) & !(TAR_BLOCK_SIZE - 1);
            offset += (TAR_BLOCK_SIZE + padded_size) as u64;
        }

        Ok(entries)
    }

    fn open_file<'c>(&'c mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'c>> {
        let (entry, _) = self.resolve_entry(path)?;
        crate::ensure!(
            !entry.is_dir(),
            FsResolverError::Invalid("Path is a directory")
        );
        Ok(Box::new(ExtentRimRead::from_contiguous(
            &mut *self.io,
            entry.data_offset,
            entry.size,
        )))
    }

    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        let (entry, _) = self.resolve_entry(path)?;
        crate::ensure!(
            entry.is_symlink(),
            FsResolverError::Invalid("Path is not a symlink")
        );
        Ok(entry.link_name)
    }

    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        let norm_path = normalize_fs_path(path);
        if norm_path.is_empty() {
            return Ok(FileAttributes::new_dir());
        }

        if let Ok((entry, _)) = self.resolve_entry(path) {
            let kind = if entry.is_dir() {
                NodeKind::Directory
            } else if entry.is_symlink() {
                NodeKind::Symlink
            } else {
                NodeKind::Regular
            };
            let mut attr = FileAttributes {
                mode: Some(entry.mode),
                uid: Some(entry.uid),
                gid: Some(entry.gid),
                kind,
                ..FileAttributes::default()
            };
            if let Ok(odt) =
                rimfs_core::time::OffsetDateTime::from_unix_timestamp(entry.mtime as i64)
            {
                attr.modified = Some(odt);
            }
            return Ok(attr);
        }

        let children = self.read_dir(path)?;
        if !children.is_empty() {
            return Ok(FileAttributes::new_dir());
        }

        Err(FsResolverError::NotFound)
    }
}
