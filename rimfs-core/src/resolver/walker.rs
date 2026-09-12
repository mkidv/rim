// SPDX-License-Identifier: MIT
//! Generic Path Walker logic to unify directory traversal across filesystems.

use crate::resolver::{FsResolverError, FsResolverResult};
use crate::utils::path_utils::split_path;

/// Trait defining the necessary operations to walk a filesystem.
/// Implemented by filesystem-specific resolvers.
pub trait WalkerDataSource {
    /// The specific Entry type returned by the filesystem (e.g. FatEntries).
    type Entry;

    /// Node identifier type (e.g. cluster number `u32` for FAT/exFAT, inode `u32` for EXT, MFT record `u64` for NTFS).
    type NodeId: Copy + Eq;

    /// Get the starting directory node (usually root).
    fn root_node(&self) -> Self::NodeId;

    /// Find a child entry by name in the given parent node.
    fn find_entry(
        &mut self,
        parent: Self::NodeId,
        name: &str,
    ) -> FsResolverResult<Option<Self::Entry>>;

    /// Check if the entry represents a directory.
    fn is_dir(&self, entry: &Self::Entry) -> bool;

    /// Get the starting node identifier of the entry (to continue traversal).
    fn entry_node(&self, entry: &Self::Entry) -> Self::NodeId;
}

/// Generic path walker.
///
/// Returns `Ok(Some(entry))` if the full path matches a valid entry.
/// Returns `Ok(None)` if the path refers to the Root (empty or "/").
/// Returns `Err` if a component is missing or not a directory.
pub fn walk_path<S: WalkerDataSource>(
    source: &mut S,
    path: &str,
) -> FsResolverResult<Option<S::Entry>> {
    if path.is_empty() || path == "/" {
        return Ok(None);
    }

    let components = split_path(path);
    let mut node = source.root_node();

    for (i, comp) in components.iter().enumerate() {
        let entry = source
            .find_entry(node, comp)?
            .ok_or(FsResolverError::NotFound)?;

        let is_last = i == components.len() - 1;

        if is_last {
            return Ok(Some(entry));
        }

        if !source.is_dir(&entry) {
            return Err(FsResolverError::Invalid(
                "Expected directory for intermediate component",
            ));
        }

        node = source.entry_node(&entry);
    }

    // Should be unreachable if components is not empty
    Err(FsResolverError::Invalid("Invalid path traversal state"))
}
