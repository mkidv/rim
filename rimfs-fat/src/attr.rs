// SPDX-License-Identifier: MIT

use crate::core::resolver::*;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct FatAttributes: u8 {
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
    fn as_fat_attr(&self) -> u8;
    fn from_fat_attr(attr: u8) -> Self;
}

impl FatFileAttributesExt for FileAttributes {
    fn as_fat_attr(&self) -> u8 {
        let mut attr = FatAttributes::empty();
        if self.read_only {
            attr |= FatAttributes::READ_ONLY;
        }
        if self.hidden {
            attr |= FatAttributes::HIDDEN;
        }
        if self.system {
            attr |= FatAttributes::SYSTEM;
        }
        if self.is_dir() {
            attr |= FatAttributes::DIRECTORY;
        }
        if self.archive {
            attr |= FatAttributes::ARCHIVE;
        }
        attr.bits()
    }

    fn from_fat_attr(attr: u8) -> Self {
        let fat_attr = FatAttributes::from_bits_truncate(attr);
        let is_dir = fat_attr.contains(FatAttributes::DIRECTORY);
        let mut fa = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };
        fa.read_only = fat_attr.contains(FatAttributes::READ_ONLY);
        fa.hidden = fat_attr.contains(FatAttributes::HIDDEN);
        fa.system = fat_attr.contains(FatAttributes::SYSTEM);
        fa.archive = fat_attr.contains(FatAttributes::ARCHIVE);
        fa
    }
}
