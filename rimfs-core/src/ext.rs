// SPDX-License-Identifier: MIT

//! Core abstractions for Ext2/3/4 filesystem components.
//!
//! This module provides traits and types to handle the differences between:
//! - **Block Maps** (Ext2/3): Direct/Indirect block addressing.
//! - **Extents** (Ext4): Tree-based extent addressing.

use crate::traits::FsMeta;

/// Metadata common to all Ext filesystems (Ext2, Ext3, Ext4)
pub trait ExtFsMeta: FsMeta<u32> {
    /// Get the block size in bytes
    fn block_size(&self) -> u32;

    /// Get the number of inodes per group
    fn inodes_per_group(&self) -> u32;
}

/// Helper trait for Ext4 Extent logic
///
/// Defines the structure and limits of Extents.
pub trait ExtentMeta: ExtFsMeta {
    /// Maximum number of extents that fit in the inode's i_block (usually 4)
    const MAX_INLINE_EXTENTS: usize = 4;

    /// Size of an extent header
    const EXTENT_HEADER_SIZE: usize = 12;

    /// Size of an extent entry
    const EXTENT_ENTRY_SIZE: usize = 12;
}

/// Helper trait for Ext2/3 Block Map logic
///
/// Defines the structure of Direct/Indirect block addressing.
pub trait BlockMapMeta: ExtFsMeta {
    /// Number of Direct Blocks in inode (usually 12)
    const DIRECT_BLOCKS: usize = 12;

    /// Index of the Indirect Block (12)
    const INDIRECT_BLOCK: usize = 12;

    /// Index of the Double Indirect Block (13)
    const DOUBLE_INDIRECT_BLOCK: usize = 13;

    /// Index of the Triple Indirect Block (14)
    const TRIPLE_INDIRECT_BLOCK: usize = 14;

    /// Number of pointers that fit in one block
    fn ptrs_per_block(&self) -> u32 {
        self.block_size() / 4
    }
}
