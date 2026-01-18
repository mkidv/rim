// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

#[cfg(feature = "std")]
pub mod std_resolver;

pub mod attr;
pub mod node;

pub use node::*;

pub use crate::core::errors::{FsResolverError, FsResolverResult};

use crate::core::utils::path_utils::*;

/// Abstraction for reading filesystem content from an external source.
///
/// This trait allows building an [`FsNode`] tree by reading a directory or a file hierarchy.
///
/// Implementations can target:
/// - The real filesystem (see [`std_resolver`])
/// - A virtual filesystem
/// - A test harness
///
/// The trait is used by the `inject()` and `check()` logic of the framework.
///
/// Notes:
/// - Paths use `/` as separator (normalized).
/// - Wildcards (`path/*`) are supported by [`Self::build_node`] and [`Self::parse_tree`].
pub trait FsResolver {
    /// Returns the list of immediate entries (files and directories) inside the given directory path.
    ///
    /// The returned names should not include path separators. They should be sorted if deterministic order is desired.
    fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>>;

    /// Returns the full content of the file at the given path.
    ///
    /// The path must refer to a regular file, not a directory.
    fn read_file(&mut self, path: &str) -> FsResolverResult<Vec<u8>>;

    /// Returns the attributes of the entry at the given path.
    ///
    /// The path may refer to a file or directory. Implementations must fill at least the `dir` flag correctly.
    fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes>;

    /// Recursively builds an [`FsNode`] tree starting from `path`.
    ///
    /// If `path` ends with `/*`, a [`FsNode::Container`] is created
    /// with all children of the base path.
    ///
    /// If `path` points to a directory:
    /// - If `recurse` is true, all subdirectories are traversed recursively.
    /// - If `recurse` is false, only the directory itself is created with no children.
    ///
    /// If `path` points to a file:
    /// - An [`FsNode::File`] is created with its content.
    ///
    /// Returns an [`FsResolverResult`] wrapping the built [`FsNode`].
    fn build_node(&mut self, path: &str, recurse: bool) -> FsResolverResult<FsNode> {
        if is_wildcard(path) {
            let base_path = strip_wildcard(path);
            let mut children = vec![];
            for entry in self.read_dir(base_path)? {
                let entry_path = join_paths(base_path, &entry);
                let child = self.build_node(&entry_path, recurse)?;
                children.push(child);
            }
            children.sort_by_key(|c| c.name().to_ascii_lowercase());
            Ok(FsNode::Container {
                children,
                attr: FileAttributes::new_dir(), // convention
            })
        } else {
            let attr = self.read_attributes(path)?;
            if attr.dir {
                let mut children = vec![];
                if recurse {
                    for entry in self.read_dir(path)? {
                        let entry_path = join_paths(path, &entry);
                        let child = self.build_node(&entry_path, recurse)?;
                        children.push(child);
                    }
                    children.sort_by_key(|c| c.name().to_ascii_lowercase());
                }
                Ok(FsNode::Dir {
                    name: extract_name_from_path(path).to_string(),
                    children,
                    attr,
                })
            } else {
                let content = self.read_file(path)?;
                Ok(FsNode::File {
                    name: extract_name_from_path(path).to_string(),
                    content,
                    attr,
                })
            }
        }
    }

    /// Parses an entire directory tree starting from `path`.
    ///
    /// Equivalent to calling [`Self::build_node`] with `recurse = true`.
    fn parse_tree(&mut self, path: &str) -> FsResolverResult<FsNode> {
        self.build_node(path, true)
    }

    /// Parses a single path (file or directory), without recursing into subdirectories.
    ///
    /// Equivalent to calling [`Self::build_node`] with `recurse = false`.
    fn parse_path(&mut self, path: &str) -> FsResolverResult<FsNode> {
        self.build_node(path, false)
    }

    fn resolve_path(&mut self, path: &str) -> FsResolverResult<(bool, u32, usize)>;
}
