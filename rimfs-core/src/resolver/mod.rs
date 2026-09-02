// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};

#[cfg(feature = "std")]
use std::boxed::Box;

#[cfg(feature = "std")]
pub mod std_resolver;

pub mod attr;
pub mod node;
pub mod walker;

pub use attr::NodeKind;
pub use node::*;

pub use crate::errors::{FsResolverError, FsResolverResult};

use crate::allocator::FsHandle;
use crate::utils::path_utils::*;
use rimio::RimRead;

/// Low-level resolver for filesystem units/resources.
///
/// Implementations handle reading specific structures (inodes, clusters, records)
/// from the underlying storage.
pub trait FsResolver<Handle: FsHandle> {
    /// Read data from the location identified by `handle`.
    fn read_unit(&mut self, handle: Handle) -> FsResolverResult<Vec<u8>>;
}

/// High-level resolver for filesystem trees (files and directories).
///
/// Symmetric counterpart to `FsTreeInjector`.
pub trait FsTreeResolver {
    /// Returns the attributes of the entry at the given path.
    ///
    /// The path may refer to a file, directory, or symlink.
    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes>;

    /// Returns the list of immediate entries (files and directories) inside the given directory path.
    ///
    /// The returned names should not include path separators.
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>>;

    /// Opens a file at the given path for streaming read without heap buffer allocation.
    ///
    /// Prefer this API for large files.
    fn open_file<'b>(&'b mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'b>>;

    /// Returns the full content of the file at the given path as a heap buffer.
    ///
    /// This intentionally materializes the whole file. Use [`FsTreeResolver::open_file`]
    /// for large files or bounded-memory pipelines.
    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>> {
        let mut stream = self.open_file(path)?;
        let size = usize::try_from(stream.total_size().map_err(FsResolverError::IO)?)
            .map_err(|_| FsResolverError::Invalid("File is too large to materialize"))?;
        #[cfg(feature = "alloc")]
        {
            let mut buf = alloc::vec![0u8; size];
            stream.read_at(0, &mut buf).map_err(FsResolverError::IO)?;
            Ok(buf)
        }
        #[cfg(not(feature = "alloc"))]
        {
            let _ = size;
            Err(FsResolverError::Unsupported)
        }
    }

    /// Returns the symbolic link target at the given path.
    fn read_link(&mut self, _path: &str) -> FsResolverResult<String> {
        Err(FsResolverError::Unsupported)
    }

    /// Resolves an entry or directory hierarchy into an owned `FsNode` snapshot.
    ///
    /// File payloads are materialized with [`FsTreeResolver::read_file`]. Use
    /// `FsTreeInjector::inject_tree_from_resolver` for bounded-memory tree injection.
    ///
    /// If `path` ends with `/*`, a `FsNode::Container` is created with all children.
    /// If `recurse` is true, subdirectories are traversed recursively.
    fn resolve_node<'node>(
        &mut self,
        path: &str,
        recurse: bool,
    ) -> FsResolverResult<FsNode<'node>> {
        if is_wildcard(path) {
            let base_path = strip_wildcard(path);
            let mut children = vec![];
            for entry in self.read_dir(base_path)? {
                let entry_path = join_paths(base_path, &entry);
                let child = self.resolve_node(&entry_path, recurse)?;
                children.push(child);
            }
            children.sort_by_key(|c| c.name().to_ascii_lowercase());
            Ok(FsNode::Container {
                children,
                attr: FileAttributes::new_dir(),
            })
        } else {
            let attr = self.read_attributes(path)?;
            match attr.kind {
                attr::NodeKind::Directory => {
                    let mut children = vec![];
                    if recurse {
                        for entry in self.read_dir(path)? {
                            let entry_path = join_paths(path, &entry);
                            let child = self.resolve_node(&entry_path, recurse)?;
                            children.push(child);
                        }
                        children.sort_by_key(|c| c.name().to_ascii_lowercase());
                    }
                    Ok(FsNode::Dir {
                        name: extract_name_from_path(path).to_string(),
                        children,
                        attr,
                    })
                }
                attr::NodeKind::Symlink => {
                    let target = self.read_link(path)?;
                    Ok(FsNode::Symlink {
                        name: extract_name_from_path(path).to_string(),
                        target,
                        attr,
                    })
                }
                _ => {
                    let bytes = self.read_file(path)?;
                    Ok(FsNode::File {
                        name: extract_name_from_path(path).to_string(),
                        source: Box::new(rimio::prelude::VecRimIO::new(bytes)),
                        attr,
                    })
                }
            }
        }
    }

    /// Resolves an entire directory tree starting from `path`.
    #[inline]
    fn resolve_tree<'node>(&mut self, path: &str) -> FsResolverResult<FsNode<'node>> {
        self.resolve_node(path, true)
    }

    /// Resolves a single path (file or directory) without recursing into subdirectories.
    #[inline]
    fn resolve_entry<'node>(&mut self, path: &str) -> FsResolverResult<FsNode<'node>> {
        self.resolve_node(path, false)
    }

    /// Checks if a path exists.
    #[inline]
    fn exists(&mut self, path: &str) -> bool {
        self.read_attributes(path).is_ok()
    }

    /// Checks if a path is a directory.
    #[inline]
    fn is_dir(&mut self, path: &str) -> bool {
        self.read_attributes(path)
            .map(|a| a.is_dir())
            .unwrap_or(false)
    }

    /// Checks if a path is a regular file.
    #[inline]
    fn is_file(&mut self, path: &str) -> bool {
        self.read_attributes(path)
            .map(|a| a.is_file())
            .unwrap_or(false)
    }
}
