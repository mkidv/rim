// SPDX-License-Identifier: MIT

//! FAT DOS file attribute flags and timestamps.

use crate::core::resolver::*;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct FatFileAttributes: u8 {
        const READ_ONLY = 0x01;
        const HIDDEN    = 0x02;
        const SYSTEM    = 0x04;
        const VOLUME_ID = 0x08;
        const DIRECTORY = 0x10;
        const ARCHIVE   = 0x20;
        const LFN       = 0x0F;
    }
}

pub trait FatFileAttributesExt {
    fn as_fat_attr(&self) -> FatFileAttributes;
    fn from_fat_attr(attr: FatFileAttributes) -> Self;
}

impl FatFileAttributesExt for FileAttributes {
    fn as_fat_attr(&self) -> FatFileAttributes {
        let mut attr = FatFileAttributes::empty();
        if self.read_only {
            attr |= FatFileAttributes::READ_ONLY;
        }
        if self.hidden {
            attr |= FatFileAttributes::HIDDEN;
        }
        if self.system {
            attr |= FatFileAttributes::SYSTEM;
        }
        if self.is_dir() {
            attr |= FatFileAttributes::DIRECTORY;
        }
        if self.archive {
            attr |= FatFileAttributes::ARCHIVE;
        }
        attr
    }

    fn from_fat_attr(attr: FatFileAttributes) -> Self {
        let is_dir = attr.contains(FatFileAttributes::DIRECTORY);
        let mut fa = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };
        fa.read_only = attr.contains(FatFileAttributes::READ_ONLY);
        fa.hidden = attr.contains(FatFileAttributes::HIDDEN);
        fa.system = attr.contains(FatFileAttributes::SYSTEM);
        fa.archive = attr.contains(FatFileAttributes::ARCHIVE);
        fa
    }
}
