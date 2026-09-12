// SPDX-License-Identifier: MIT

//! ext2/3/4 specification constants, magic numbers, and creator tags.

#![allow(dead_code)]

// Creator Tag (stored at end of s_reserved in superblock)

pub const EXT_CREATOR_TAG: &[u8; 8] = b"RIM     ";

/// Returns the creator tag with version appended (e.g., "RIM0.1.0")
pub fn oem_name() -> [u8; 8] {
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    let mut out = *EXT_CREATOR_TAG;
    let ver = VERSION.as_bytes();
    let mut i = 0;
    while i < ver.len() && 3 + i < 8 {
        out[3 + i] = ver[i];
        i += 1;
    }
    out
}

// Superblock

// Magic number EXT (in s_magic)
pub const EXT_SUPERBLOCK_MAGIC: u16 = 0xEF53;

// Superblock size (in logical memory)
pub const EXT_SUPERBLOCK_SIZE: usize = 1024;

pub const EXT_SUPERBLOCK_OFFSET: u64 = 1024;

// Logical block where the superblock is located (in disk image)
pub const EXT_SUPERBLOCK_BLOCK_NUMBER: u32 = 0;

// Block Size

// Default value
pub const EXT_DEFAULT_BLOCK_SIZE: u32 = 4096;

// Minimum / maximum allowed size (EXT spec)
pub const EXT_MIN_BLOCK_SIZE: u32 = 1024;
pub const EXT_MAX_BLOCK_SIZE: u32 = 65536;

// Inode

pub const EXT_ROOT_INODE: u32 = 2;
pub const EXT_FIRST_INODE: u32 = 11;
pub const EXT_DEFAULT_INODE_SIZE: u32 = 256; // Can be 128 or 256
pub const EXT2_DEFAULT_INODE_SIZE: u32 = 128;
pub const EXT_MIN_INODE_SIZE: usize = 128;
pub const EXT_MAX_INODE_SIZE: usize = 1024;
pub const EXT_DEFAULT_BYTES_PER_INODE: u64 = 16 * 1024;

// Block Groups

pub const EXT_DEFAULT_BLOCKS_PER_GROUP: u32 = EXT_DEFAULT_BLOCK_SIZE * 8;

pub const EXT_DEFAULT_INODES_PER_GROUP: u32 = ((EXT_DEFAULT_BLOCKS_PER_GROUP as u64
    * EXT_DEFAULT_BLOCK_SIZE as u64)
    / EXT_DEFAULT_BYTES_PER_INODE) as u32;

// BGDT entry size
pub const EXT2_BGDT_ENTRY_SIZE: usize = 32;
pub const EXT4_BGDT_ENTRY_SIZE: usize = 64;
pub const EXT_BGDT_ENTRY_SIZE: usize = EXT4_BGDT_ENTRY_SIZE;

// Default UID / GID

pub const EXT_DEFAULT_UID: u16 = 0;
pub const EXT_DEFAULT_GID: u16 = 0;

// Inode Flags

// Inode uses EXTENTS (modern mode)
pub const EXT_INODE_FLAG_EXTENTS: u32 = 0x0008_0000;

// Directory with hash index (dir_index feature)
pub const EXT_INODE_FLAG_INDEX: u32 = 0x0001_0000;

// Immutable file
pub const EXT_INODE_FLAG_IMMUTABLE: u32 = 0x0000_0010;

// Journal

// Default journal size (number of blocks)
pub const EXT_DEFAULT_JOURNAL_BLOCKS: u32 = 1024;

// Filesystem Features (Raw constants used by superblock/checker)
pub const EXT_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
pub const EXT_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;

// Backup Groups

pub const EXT_BACKUP_GROUPS: u32 = 2;

// Miscellaneous

// Extent header magic number
pub const EXT_EXTENT_HEADER_MAGIC: u16 = 0xF30A;

// End of block list value in EXTENTS
pub const EXT_EXTENT_EOF: u32 = 0xFFFFFFFF;
pub const EXT_ROOT_DIR_LINKS_COUNT: u16 = 2;

// Directory Entry File Types
pub const EXT_FT_UNKNOWN: u8 = 0;
pub const EXT_FT_REG_FILE: u8 = 1;
pub const EXT_FT_DIR: u8 = 2;
pub const EXT_FT_CHRDEV: u8 = 3;
pub const EXT_FT_BLKDEV: u8 = 4;
pub const EXT_FT_FIFO: u8 = 5;
pub const EXT_FT_SOCK: u8 = 6;
pub const EXT_FT_SYMLINK: u8 = 7;
