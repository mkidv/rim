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
    entries: Option<Vec<(TarEntry<'static>, u64)>>,
}

impl<'a, IO: RimRead + ?Sized> TarResolver<'a, IO> {
    /// Creates a new `TarResolver` over an existing storage stream and metadata.
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> Self {
        Self {
            io,
            _meta: meta,
            entries: None,
        }
    }

    /// Read and parse a raw TAR header at the given byte offset.
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

        let mut full_name = String::new();
        if &header[257..263] == USTAR_MAGIC {
            let mut prefix_end = 0;
            while prefix_end < 155 && header[345 + prefix_end] != 0 {
                prefix_end += 1;
            }
            if prefix_end > 0
                && let Ok(prefix_str) = core::str::from_utf8(&header[345..345 + prefix_end])
            {
                full_name.push_str(prefix_str);
                if !prefix_str.ends_with('/') {
                    full_name.push('/');
                }
            }
        }
        full_name.push_str(name_str);

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
        let entry_name = normalize_fs_path(&full_name);

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

    /// Builds or returns the cached single-pass index of all TAR entries.
    fn ensure_index(&mut self) -> FsResolverResult<&[(TarEntry<'static>, u64)]> {
        if self.entries.is_none() {
            let mut list = Vec::new();
            let mut offset = 0u64;
            let mut pending_long_name: Option<String> = None;
            let mut pending_long_link: Option<String> = None;

            while let Some(entry) = self.read_header_at(offset)? {
                let padded_size =
                    (entry.size as usize + TAR_BLOCK_SIZE - 1) & !(TAR_BLOCK_SIZE - 1);
                let next_offset = offset + (TAR_BLOCK_SIZE + padded_size) as u64;

                if entry.typeflag == GNULONGNAME {
                    let mut buf = alloc::vec![0u8; entry.size as usize];
                    self.io
                        .read_at(entry.data_offset, &mut buf)
                        .map_err(FsResolverError::IO)?;
                    while buf.last() == Some(&0) {
                        buf.pop();
                    }
                    if let Ok(s) = String::from_utf8(buf) {
                        pending_long_name = Some(s);
                    }
                    offset = next_offset;
                    continue;
                } else if entry.typeflag == GNULONGLINK_TARGET {
                    let mut buf = alloc::vec![0u8; entry.size as usize];
                    self.io
                        .read_at(entry.data_offset, &mut buf)
                        .map_err(FsResolverError::IO)?;
                    while buf.last() == Some(&0) {
                        buf.pop();
                    }
                    if let Ok(s) = String::from_utf8(buf) {
                        pending_long_link = Some(s);
                    }
                    offset = next_offset;
                    continue;
                }

                let mut actual_entry = entry;
                if let Some(name) = pending_long_name.take() {
                    actual_entry.name = normalize_fs_path(&name).to_string();
                }
                if let Some(link) = pending_long_link.take() {
                    actual_entry.link_name = link;
                }

                list.push((actual_entry, offset));
                offset = next_offset;
            }
            self.entries = Some(list);
        }
        Ok(self.entries.as_ref().unwrap())
    }

    /// Resolves an entry by looking up in the indexed entries.
    pub fn resolve_entry(&mut self, path: &str) -> FsResolverResult<(TarEntry<'static>, u64)> {
        let norm_path = normalize_fs_path(path);
        let entries = self.ensure_index()?;
        for (entry, offset) in entries {
            let entry_name = normalize_fs_path(&entry.name);
            if entry_name == norm_path
                || (entry.is_dir()
                    && entry_name.trim_end_matches('/') == norm_path.trim_end_matches('/'))
            {
                return Ok((entry.clone(), *offset));
            }
        }

        Err(FsResolverError::NotFound)
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for TarResolver<'a, IO> {
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let norm_path = normalize_fs_path(path);
        let entries = self.ensure_index()?;
        let mut results = Vec::new();

        for (entry, _) in entries {
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
                if !first_comp.is_empty() && !results.contains(&first_comp.to_string()) {
                    results.push(first_comp.to_string());
                }
            }
        }

        Ok(results)
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
        if !children.is_empty() || self.has_descendant(norm_path)? {
            return Ok(FileAttributes::new_dir());
        }

        Err(FsResolverError::NotFound)
    }
}

impl<'a, IO: RimRead + ?Sized> TarResolver<'a, IO> {
    fn has_descendant(&mut self, norm_path: &str) -> FsResolverResult<bool> {
        let prefix = if norm_path.is_empty() {
            String::new()
        } else {
            let mut prefix = norm_path.to_string();
            prefix.push('/');
            prefix
        };
        let entries = self.ensure_index()?;
        for (entry, _) in entries {
            let entry_name = normalize_fs_path(&entry.name);
            if !entry_name.is_empty() && entry_name.starts_with(&prefix) {
                return Ok(true);
            }
        }

        Ok(false)
    }
}
