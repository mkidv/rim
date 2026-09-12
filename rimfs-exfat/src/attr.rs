// SPDX-License-Identifier: MIT

//! exFAT file attributes and timestamps.

// rimgen/fs/exfat/attr.rs

use crate::core::resolver::attr::FileAttributes;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct ExFatAttributes: u16 {
        const READ_ONLY = 0x0001;
        const HIDDEN    = 0x0002;
        const SYSTEM    = 0x0004;
        const DIRECTORY = 0x0010;
        const ARCHIVE   = 0x0020;
    }
}

pub trait ExFatFileAttributesExt {
    fn as_exfat_attr(&self) -> u16;
    fn from_exfat_attr(attr: u16) -> Self;
}

impl ExFatFileAttributesExt for FileAttributes {
    fn as_exfat_attr(&self) -> u16 {
        let mut attr = ExFatAttributes::empty();
        if self.read_only {
            attr |= ExFatAttributes::READ_ONLY;
        }
        if self.hidden {
            attr |= ExFatAttributes::HIDDEN;
        }
        if self.system {
            attr |= ExFatAttributes::SYSTEM;
        }
        if self.is_dir() {
            attr |= ExFatAttributes::DIRECTORY;
        }
        if self.archive {
            attr |= ExFatAttributes::ARCHIVE;
        }
        attr.bits()
    }

    fn from_exfat_attr(attr: u16) -> Self {
        let exfat_attr = ExFatAttributes::from_bits_truncate(attr);
        let is_dir = exfat_attr.contains(ExFatAttributes::DIRECTORY);
        let mut fa = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };
        fa.read_only = exfat_attr.contains(ExFatAttributes::READ_ONLY);
        fa.hidden = exfat_attr.contains(ExFatAttributes::HIDDEN);
        fa.system = exfat_attr.contains(ExFatAttributes::SYSTEM);
        fa.archive = exfat_attr.contains(ExFatAttributes::ARCHIVE);
        fa
    }
}
