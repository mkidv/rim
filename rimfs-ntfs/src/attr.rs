// SPDX-License-Identifier: MIT
//! NTFS file attributes helpers

pub use crate::types::record::{AttributeType, NtfsFileNameNamespace};
pub use crate::utils::current_ntfs_time;

use crate::core::resolver::attr::FileAttributes;
use crate::flags::*;

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
