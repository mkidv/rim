// SPDX-License-Identifier: MIT

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
use std::{fs, io::Error, path::Path};

use rimio::errors::RimIOError;

use crate::{
    resolver::*,
    utils::{path_utils::*, time_utils::*},
};

/// Standard filesystem parser implementation of [`FsTreeResolver`] using the local filesystem.
///
/// This parser operates on the real filesystem using `std::fs`, and implements the expected behavior
/// of [`FsTreeResolver`] for injection and checking.
///
/// Paths are normalized with `/` separators.
/// This implementation is only available when the `std` feature is enabled.
#[cfg(feature = "std")]
#[derive(Debug, Clone, Default)]
pub struct StdResolver {}

#[cfg(feature = "std")]
impl StdResolver {
    /// Creates a new [`StdResolver`].
    pub fn new() -> Self {
        Self {}
    }
}

#[cfg(feature = "std")]
impl FsTreeResolver for StdResolver {
    /// Returns the list of immediate entries (files and directories) inside the given directory path.
    ///
    /// The returned names are file names only (without path), and are sorted for deterministic output.
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(&path_str);
        let mut res = vec![];
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                res.push(name.to_string());
            } else {
                return Err(FsResolverError::Unsupported);
            }
        }
        res.sort_unstable();
        Ok(res)
    }

    /// Opens a file at the given path for streaming read without heap buffer allocation.
    fn open_file<'b>(&'b mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'b>> {
        let path_str = clean_and_normalize_path(path);
        let file = fs::File::open(&path_str)?;
        let io = rimio::prelude::ReadOnlyFileRimIO::from_file(file)?;
        Ok(Box::new(io))
    }

    /// Returns the full content of the file at the given path.
    ///
    /// The path must refer to a regular file, not a directory.
    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(&path_str);
        Ok(fs::read(path)?)
    }

    /// Returns the symbolic link target at the given path.
    fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(&path_str);
        let target = fs::read_link(path)?;
        target
            .into_os_string()
            .into_string()
            .map_err(|_| FsResolverError::Invalid("Symlink target is not valid UTF-8"))
    }

    /// Returns the attributes of the entry at the given path.
    ///
    /// The path may refer to a file, directory, or symlink.
    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(path_str.as_str());
        let meta = fs::symlink_metadata(path)?;

        let file_type = meta.file_type();
        let kind = if file_type.is_dir() {
            NodeKind::Directory
        } else if file_type.is_symlink() {
            NodeKind::Symlink
        } else if file_type.is_file() {
            NodeKind::Regular
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::FileTypeExt;
                if file_type.is_fifo() {
                    NodeKind::Fifo
                } else if file_type.is_socket() {
                    NodeKind::Socket
                } else if file_type.is_char_device() {
                    NodeKind::CharDevice
                } else if file_type.is_block_device() {
                    NodeKind::BlockDevice
                } else {
                    NodeKind::Regular
                }
            }
            #[cfg(not(unix))]
            {
                NodeKind::Regular
            }
        };

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        #[cfg(unix)]
        let (mode, uid, gid) = {
            use std::os::unix::fs::MetadataExt;
            (Some(meta.mode()), Some(meta.uid()), Some(meta.gid()))
        };
        #[cfg(not(unix))]
        let (mode, uid, gid) = (None, None, None);

        Ok(FileAttributes {
            read_only: meta.permissions().readonly(),
            hidden: name.starts_with('.'),
            // No portable way to detect SYSTEM attribute cross-platform
            system: false,
            archive: kind.is_file(),
            kind,
            created: meta.created().ok().map(systemtime_to_offsetdatetime),
            modified: meta.modified().ok().map(systemtime_to_offsetdatetime),
            accessed: meta.accessed().ok().map(systemtime_to_offsetdatetime),
            contiguous: false,
            mode,
            uid,
            gid,
        })
    }
}

#[cfg(feature = "std")]
impl From<Error> for FsResolverError {
    fn from(e: Error) -> Self {
        FsResolverError::IO(RimIOError::from(e))
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_current_dir_parser() {
        let mut parser = StdResolver::new();
        let root = ".";

        let entries = parser.read_dir(root).unwrap();
        assert!(!entries.is_empty(), "Root dir should not be empty");

        for name in &entries {
            let path = format!("{root}/{name}");
            let attr = parser.read_attributes(&path).unwrap();
            if attr.is_dir() {
                println!("DIR: {name}");
            } else {
                let content = parser.read_file(&path).unwrap_or_default();
                println!("FILE: {name} ({} bytes)", content.len());
            }
        }
    }
}
