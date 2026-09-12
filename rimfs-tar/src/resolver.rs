// SPDX-License-Identifier: MIT

//! TAR archive sequential reader and path resolver.

use rimio::RimReadStructExt;

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
use rimfs_core::resolver::{FsTreeResolver, PathIndex, attr::FileAttributes, attr::NodeKind};
use rimio::RimRead;
use rimio::prelude::*;

/// Filesystem resolver for TAR archives and streams.
///
/// Implements [`FsTreeResolver`] over any [`RimRead`] storage stream.
pub struct TarResolver<'a, IO: RimRead + ?Sized> {
    io: &'a mut IO,
    _meta: &'a TarMeta,
    index: Option<PathIndex<(TarEntry<'static>, u64)>>,
}

impl<'a, IO: RimRead + ?Sized> TarResolver<'a, IO> {
    /// Creates a new `TarResolver` over an existing storage stream and metadata.
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> Self {
        Self {
            io,
            _meta: meta,
            index: None,
        }
    }

    /// Read and parse a raw TAR header at the given byte offset.
    fn read_header_at(&mut self, offset: u64) -> FsResolverResult<Option<TarEntry<'static>>> {
        let header: UstarHeader = self.io.read_struct(offset)?;
        if header.is_zero() {
            let mut trailer = [0u8; TAR_BLOCK_SIZE];
            self.io.read_at(
                offset
                    .checked_add(TAR_BLOCK_SIZE as u64)
                    .ok_or(FsResolverError::Invalid("Archive offset overflow"))?,
                &mut trailer,
            )?;
            if trailer.iter().any(|b| *b != 0) {
                return Err(FsResolverError::Invalid("Invalid TAR trailer"));
            }
            return Ok(None);
        }
        if header.calculate_checksum() as u64 != parse_octal(&header.checksum) {
            return Err(FsResolverError::Invalid("TAR checksum mismatch"));
        }

        let mut name_end = 0;
        while name_end < 100 && header.name[name_end] != 0 {
            name_end += 1;
        }
        let name_str = core::str::from_utf8(&header.name[..name_end])
            .map_err(|_| FsResolverError::Unsupported)?;

        let mut full_name = String::new();
        if &header.magic == USTAR_MAGIC {
            let mut prefix_end = 0;
            while prefix_end < 155 && header.prefix[prefix_end] != 0 {
                prefix_end += 1;
            }
            if prefix_end > 0
                && let Ok(prefix_str) = core::str::from_utf8(&header.prefix[..prefix_end])
            {
                full_name.push_str(prefix_str);
                if !prefix_str.ends_with('/') {
                    full_name.push('/');
                }
            }
        }
        full_name.push_str(name_str);

        let mode = parse_octal(&header.mode) as u32;
        let uid = parse_octal(&header.uid) as u32;
        let gid = parse_octal(&header.gid) as u32;
        let size = parse_octal(&header.size);
        let mtime = parse_octal(&header.mtime);
        let typeflag = header.typeflag;
        if !matches!(typeflag, 0 | b'0' | b'2' | b'5' | b'L' | b'K') {
            return Err(FsResolverError::Unsupported);
        }
        let end = offset
            .checked_add(TAR_BLOCK_SIZE as u64)
            .and_then(|o| o.checked_add(size))
            .ok_or(FsResolverError::Invalid("TAR payload overflow"))?;
        if end > self.io.total_size()? {
            return Err(FsResolverError::Invalid("Truncated TAR payload"));
        }

        let mut link_end = 0;
        while link_end < 100 && header.link_name[link_end] != 0 {
            link_end += 1;
        }
        let link_str = core::str::from_utf8(&header.link_name[..link_end])
            .map_err(|_| FsResolverError::Unsupported)?;

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

    /// Scan validated entries without retaining a path index (used by the checker).
    pub(crate) fn scan_entries(
        &mut self,
        mut visit: impl FnMut(TarEntry<'static>, u64),
    ) -> FsResolverResult<()> {
        let mut offset = 0u64;
        let mut pending_long_name: Option<String> = None;
        let mut pending_long_link: Option<String> = None;

        while let Some(entry) = self.read_header_at(offset)? {
            let padded_size = entry
                .size
                .checked_add(511)
                .map(|v| v & !511)
                .ok_or(FsResolverError::Invalid("TAR size overflow"))?;
            let next_offset = offset
                .checked_add(512)
                .and_then(|v| v.checked_add(padded_size))
                .ok_or(FsResolverError::Invalid("TAR offset overflow"))?;
            if matches!(entry.typeflag, GNULONGNAME | GNULONGLINK_TARGET) && entry.size > 65536 {
                return Err(FsResolverError::Invalid("TAR extended name exceeds limit"));
            }

            if entry.typeflag == GNULONGNAME {
                let mut buf = alloc::vec![0u8; entry.size as usize];
                self.io
                    .read_at(entry.data_offset, &mut buf)
                    .map_err(FsResolverError::IO)?;
                while buf.last() == Some(&0) {
                    buf.pop();
                }
                pending_long_name =
                    Some(String::from_utf8(buf).map_err(|_| FsResolverError::Unsupported)?);
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
                pending_long_link =
                    Some(String::from_utf8(buf).map_err(|_| FsResolverError::Unsupported)?);
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

            visit(actual_entry, offset);
            offset = next_offset;
        }
        if pending_long_name.is_some() || pending_long_link.is_some() {
            return Err(FsResolverError::Invalid("Orphan TAR extended header"));
        }
        Ok(())
    }

    /// Builds or returns the cached single-pass index of all TAR entries.
    fn ensure_index(&mut self) -> FsResolverResult<&PathIndex<(TarEntry<'static>, u64)>> {
        if self.index.is_none() {
            let mut index = PathIndex::new();
            self.scan_entries(|entry, offset| {
                let name = entry.name.clone();
                let is_dir = entry.is_dir();
                index.insert_with_kind(&name, (entry, offset), is_dir);
            })?;
            self.index = Some(index);
        }
        Ok(self.index.as_ref().unwrap())
    }

    /// Resolves an entry by looking up in the indexed entries.
    pub fn resolve_entry(&mut self, path: &str) -> FsResolverResult<(TarEntry<'static>, u64)> {
        self.ensure_index()?;
        self.index
            .as_ref()
            .unwrap()
            .get(path)
            .cloned()
            .ok_or(FsResolverError::NotFound)
    }
}

impl<'a, IO: RimRead + ?Sized> FsTreeResolver for TarResolver<'a, IO> {
    fn exists(&mut self, path: &str) -> bool {
        if self.ensure_index().is_err() {
            return false;
        }
        self.index.as_ref().unwrap().contains_path(path)
    }

    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let norm_path = normalize_fs_path(path);
        let trimmed = norm_path.trim_end_matches('/');
        self.ensure_index()?;
        let index = self.index.as_ref().unwrap();
        if index.get(trimmed).is_some_and(|(entry, _)| !entry.is_dir()) {
            return Err(FsResolverError::Invalid("Path is not a directory"));
        }
        if !index.is_dir(trimmed) {
            return Err(FsResolverError::NotFound);
        }
        Ok(index.children(trimmed).unwrap_or_default())
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
        let trimmed = norm_path.trim_end_matches('/');
        if trimmed.is_empty() {
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
                read_only: (entry.mode & 0o222) == 0,
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

        self.ensure_index()?;
        if self.index.as_ref().unwrap().is_dir(trimmed) {
            return Ok(FileAttributes::new_dir());
        }

        Err(FsResolverError::NotFound)
    }
}
