// SPDX-License-Identifier: MIT

/// Common statistics collected during a directory tree walk.
#[derive(Debug, Default, Clone, Copy)]
pub struct WalkerStats {
    /// Number of directories visited.
    pub dirs_visited: usize,
    /// Number of files found.
    pub files_found: usize,
    /// Number of directory entries scanned (filesystem specific).
    pub entries_scanned: usize,
    /// Maximum directory depth reached.
    pub max_depth: usize,
    /// Number of inodes checked (specific to inode-based filesystems like EXT4).
    pub inodes_checked: usize,
}

impl WalkerStats {
    pub fn new() -> Self {
        Self::default()
    }
}
