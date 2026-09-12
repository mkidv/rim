// SPDX-License-Identifier: MIT
//! NTFS file attributes helpers
use crate::core::resolver::attr::FileAttributes;

bitflags::bitflags! {
    /// File Attributes (DOS/Windows style)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct NtfsFileAttributes: u32 {
        const READ_ONLY = 0x0001;
        const HIDDEN    = 0x0002;
        const SYSTEM    = 0x0004;
        const DIRECTORY = 0x0010;
        const ARCHIVE   = 0x0020;
        const DEVICE    = 0x0040;
        const NORMAL    = 0x0080;
        const TEMPORARY = 0x0100;
        const SPARSE    = 0x0200;
        const REPARSE   = 0x0400;
        const COMPRESSED = 0x0800;
        const OFFLINE   = 0x1000;
        const ENCRYPTED = 0x4000;
        const I30_INDEX = 0x1000_0000;
        const VIEW_INDEX = 0x2000_0000;
    }
}

pub trait NtfsFileAttributesExt {
    fn as_ntfs_attr(&self) -> NtfsFileAttributes;
    fn from_ntfs_attr(attr: NtfsFileAttributes) -> Self;
}

impl NtfsFileAttributesExt for FileAttributes {
    fn as_ntfs_attr(&self) -> NtfsFileAttributes {
        let mut flags = NtfsFileAttributes::empty();
        if self.read_only {
            flags |= NtfsFileAttributes::READ_ONLY;
        }
        if self.hidden {
            flags |= NtfsFileAttributes::HIDDEN;
        }
        if self.system {
            flags |= NtfsFileAttributes::SYSTEM;
        }
        if self.archive {
            flags |= NtfsFileAttributes::ARCHIVE;
        }
        if self.is_dir() {
            flags |= NtfsFileAttributes::DIRECTORY;
        }
        flags
    }

    fn from_ntfs_attr(attr: NtfsFileAttributes) -> Self {
        let is_dir = attr.contains(NtfsFileAttributes::DIRECTORY);
        let mut fa = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };
        fa.read_only = attr.contains(NtfsFileAttributes::READ_ONLY);
        fa.hidden = attr.contains(NtfsFileAttributes::HIDDEN);
        fa.system = attr.contains(NtfsFileAttributes::SYSTEM);
        fa.archive = attr.contains(NtfsFileAttributes::ARCHIVE);
        fa
    }
}
