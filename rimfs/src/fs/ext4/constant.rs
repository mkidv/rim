// SPDX-License-Identifier: MIT
// rimgen/fs/ext4/constant.rs

// === Superblock ===

// Magic number EXT4 (in s_magic)
pub const EXT4_SUPERBLOCK_MAGIC: u16 = 0xEF53;

// Superblock size (in logical memory)
pub const EXT4_SUPERBLOCK_SIZE: usize = 1024;

pub const EXT4_SUPERBLOCK_OFFSET: u64 = 1024;

// Logical block where the superblock is located (in disk image)
pub const EXT4_SUPERBLOCK_BLOCK_NUMBER: u32 = 0;

// === Block Size ===

// Default value
pub const EXT4_DEFAULT_BLOCK_SIZE: u32 = 4096;

// Minimum / maximum allowed size (EXT4 spec)
pub const EXT4_MIN_BLOCK_SIZE: u32 = 1024;
pub const EXT4_MAX_BLOCK_SIZE: u32 = 65536;

// === Inode ===

pub const EXT4_ROOT_INODE: u32 = 2;
pub const EXT4_FIRST_INODE: u32 = 11;
pub const EXT4_DEFAULT_INODE_SIZE: u32 = 256; // Can be 128 or 256
pub const EXT4_MIN_INODE_SIZE: usize = 128;
pub const EXT4_MAX_INODE_SIZE: usize = 1024;

// === Block Groups ===

pub const EXT4_DEFAULT_BLOCKS_PER_GROUP: u32 = 8192;
pub const EXT4_DEFAULT_INODES_PER_GROUP: u32 = 256;

// BGDT entry size
pub const EXT4_BGDT_ENTRY_SIZE: usize = 64;

// === Default UID / GID ===

pub const EXT4_DEFAULT_UID: u16 = 0;
pub const EXT4_DEFAULT_GID: u16 = 0;

// === Inode Flags ===

// Inode uses EXTENTS (modern mode)
pub const EXT4_INODE_FLAG_EXTENTS: u32 = 0x0008_0000;

// Directory with hash index (dir_index feature)
pub const EXT4_INODE_FLAG_INDEX: u32 = 0x0001_0000;

// Immutable file
pub const EXT4_INODE_FLAG_IMMUTABLE: u32 = 0x0000_0010;

// === Journal ===

// Default journal size (number of blocks)
pub const EXT4_DEFAULT_JOURNAL_BLOCKS: u32 = 1024;

// === Filesystem Features (Superblock flags) ===

// Compatible features
pub const EXT4_FEATURE_COMPAT_DIR_PREALLOC: u32 = 0x0001;
pub const EXT4_FEATURE_COMPAT_IMAGIC_INODES: u32 = 0x0002;
pub const EXT4_FEATURE_COMPAT_HAS_JOURNAL: u32 = 0x0004;
pub const EXT4_FEATURE_COMPAT_EXT_ATTR: u32 = 0x0008;
pub const EXT4_FEATURE_COMPAT_RESIZE_INODE: u32 = 0x0010;
pub const EXT4_FEATURE_COMPAT_DIR_INDEX: u32 = 0x0020;

// Incompatible features
pub const EXT4_FEATURE_INCOMPAT_COMPRESSION: u32 = 0x0001;
pub const EXT4_FEATURE_INCOMPAT_FILETYPE: u32 = 0x0002;
pub const EXT4_FEATURE_INCOMPAT_RECOVER: u32 = 0x0004;
pub const EXT4_FEATURE_INCOMPAT_JOURNAL_DEV: u32 = 0x0008;
pub const EXT4_FEATURE_INCOMPAT_META_BG: u32 = 0x0010;
pub const EXT4_FEATURE_INCOMPAT_EXTENTS: u32 = 0x0040;
pub const EXT4_FEATURE_INCOMPAT_64BIT: u32 = 0x0080;
pub const EXT4_FEATURE_INCOMPAT_MMP: u32 = 0x0100;
pub const EXT4_FEATURE_INCOMPAT_FLEX_BG: u32 = 0x0200;
pub const EXT4_FEATURE_INCOMPAT_EA_INODE: u32 = 0x0400;
pub const EXT4_FEATURE_INCOMPAT_DIRDATA: u32 = 0x1000;

// Read-only compatible features
pub const EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER: u32 = 0x0001;
pub const EXT4_FEATURE_RO_COMPAT_LARGE_FILE: u32 = 0x0002;
pub const EXT4_FEATURE_RO_COMPAT_BTREE_DIR: u32 = 0x0004;
pub const EXT4_FEATURE_RO_COMPAT_HUGE_FILE: u32 = 0x0008;
pub const EXT4_FEATURE_RO_COMPAT_GDT_CSUM: u32 = 0x0010;
pub const EXT4_FEATURE_RO_COMPAT_DIR_NLINK: u32 = 0x0020;
pub const EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE: u32 = 0x0040;

// === Backup Groups ===

pub const EXT4_BACKUP_GROUPS: u32 = 2;

// === Miscellaneous ===

// End of block list value in EXTENTS
pub const EXT4_EXTENT_EOF: u32 = 0xFFFFFFFF;
pub const EXT4_ROOT_DIR_LINKS_COUNT: u16 = 2;
