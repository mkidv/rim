// SPDX-License-Identifier: MIT

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
use std::{fs, io::Error, path::Path};

use rimio::errors::RimIOError;

use crate::core::{
    resolver::*,
    utils::{path_utils::*, time_utils::*},
};

/// Standard filesystem parser implementation of [`FsResolver`] using the local filesystem.
///
/// This parser operates on the real filesystem using `std::fs`, and implements the expected behavior
/// of [`FsResolver`] for injection and checking.
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
impl FsResolver for StdResolver {
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

    /// Returns the full content of the file at the given path.
    ///
    /// The path must refer to a regular file, not a directory.
    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(&path_str);
        Ok(fs::read(path)?)
    }

    /// Returns the attributes of the entry at the given path.
    ///
    /// The path may refer to a file or directory.
    /// Implementations must fill at least the `dir` flag correctly.
    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(path_str.as_str());
        let meta = fs::metadata(path)?;
        if !meta.is_file() && !meta.is_dir() {
            crate::bail!(FsResolverError::Unsupported);
        }

        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(FsResolverError::Unsupported)?;

        Ok(FileAttributes {
            read_only: meta.permissions().readonly(),
            hidden: name.starts_with('.'),
            // No portable way to detect SYSTEM attribute cross-platform
            system: false,
            archive: !meta.is_dir(),
            dir: meta.is_dir(),
            created: meta.created().ok().map(systemtime_to_offsetdatetime),
            modified: meta.modified().ok().map(systemtime_to_offsetdatetime),
            accessed: meta.accessed().ok().map(systemtime_to_offsetdatetime),
            mode: {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    Some(meta.permissions().mode())
                }
                #[cfg(not(unix))]
                {
                    None
                }
            },
        })
    }

    fn resolve_path(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)> {
        let path_str = clean_and_normalize_path(path);
        let path = Path::new(&path_str);

        let meta = fs::metadata(path)?;

        let is_dir = meta.is_dir();
        let size = if meta.is_file() {
            meta.len() as usize
        } else {
            0
        };

        Ok((is_dir, 0, size)) // handle/unit = 0 (not used here)
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
            if attr.dir {
                println!("DIR: {name}");
            } else {
                let content = parser.read_file(&path).unwrap_or_default();
                println!("FILE: {name} ({} bytes)", content.len());
            }
        }
    }
}
