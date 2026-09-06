// SPDX-License-Identifier: MIT
//! Typed bitflags and enumerations for EXT filesystems.

use crate::core::traits::NodeKind;
use bitflags::bitflags;

bitflags! {
    /// Compatible feature flags for the EXT superblock (`s_feature_compat`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ExtCompatFeatures: u32 {
        const DIR_PREALLOC   = 0x0001;
        const IMAGIC_INODES  = 0x0002;
        const HAS_JOURNAL    = 0x0004;
        const EXT_ATTR       = 0x0008;
        const RESIZE_INODE   = 0x0010;
        const DIR_INDEX      = 0x0020;
    }
}

bitflags! {
    /// Incompatible feature flags for the EXT superblock (`s_feature_incompat`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ExtIncompatFeatures: u32 {
        const COMPRESSION = 0x0001;
        const FILETYPE    = 0x0002;
        const RECOVER     = 0x0004;
        const JOURNAL_DEV = 0x0008;
        const META_BG     = 0x0010;
        const EXTENTS     = 0x0040;
        const _64BIT      = 0x0080;
        const MMP         = 0x0100;
        const FLEX_BG     = 0x0200;
        const EA_INODE    = 0x0400;
        const DIRDATA     = 0x1000;
    }
}

bitflags! {
    /// Read-only compatible feature flags for the EXT superblock (`s_feature_ro_compat`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ExtRoCompatFeatures: u32 {
        const SPARSE_SUPER = 0x0001;
        const LARGE_FILE   = 0x0002;
        const BTREE_DIR    = 0x0004;
        const HUGE_FILE    = 0x0008;
        const GDT_CSUM     = 0x0010;
        const DIR_NLINK    = 0x0020;
        const EXTRA_ISIZE  = 0x0040;
    }
}

bitflags! {
    /// Inode flags (`i_flags`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct ExtInodeFlags: u32 {
        const SECRM        = 0x0000_0001;
        const UNRM         = 0x0000_0002;
        const COMPR        = 0x0000_0004;
        const SYNC         = 0x0000_0008;
        const IMMUTABLE    = 0x0000_0010;
        const APPEND       = 0x0000_0020;
        const NODUMP       = 0x0000_0040;
        const NOATIME      = 0x0000_0080;
        const DIRTY        = 0x0000_0100;
        const COMPRBLK     = 0x0000_0200;
        const NOCOMPR      = 0x0000_0400;
        const ECOMPR       = 0x0000_0800;
        const INDEX        = 0x0001_0000;
        const IMAGIC       = 0x0002_0000;
        const JOURNAL_DATA = 0x0004_0000;
        const NOTAIL       = 0x0008_0000;
        const EXTENTS      = 0x0008_0000; // In modern ext4, 0x80000 is extents
        const DIRSYNC      = 0x0001_0000;
        const TOPDIR       = 0x0002_0000;
        const HUGE_FILE    = 0x0004_0000;
        const EA_INODE     = 0x0020_0000;
        const EOFBLOCKS    = 0x0040_0000;
    }
}

/// Directory entry file types according to EXT specs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExtFileType {
    Unknown = 0,
    RegularFile = 1,
    Directory = 2,
    CharacterDevice = 3,
    BlockDevice = 4,
    Fifo = 5,
    Socket = 6,
    Symlink = 7,
}

impl ExtFileType {
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    #[inline]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::RegularFile,
            2 => Self::Directory,
            3 => Self::CharacterDevice,
            4 => Self::BlockDevice,
            5 => Self::Fifo,
            6 => Self::Socket,
            7 => Self::Symlink,
            _ => Self::Unknown,
        }
    }
}

impl From<NodeKind> for ExtFileType {
    fn from(kind: NodeKind) -> Self {
        match kind {
            NodeKind::Directory => Self::Directory,
            NodeKind::Symlink => Self::Symlink,
            NodeKind::Regular => Self::RegularFile,
            NodeKind::CharDevice => Self::CharacterDevice,
            NodeKind::BlockDevice => Self::BlockDevice,
            NodeKind::Fifo => Self::Fifo,
            NodeKind::Socket => Self::Socket,
        }
    }
}
