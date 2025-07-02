// SPDX-License-Identifier: MIT
// rimgen/fs/exfat/attr.rs

use crate::core::parser::attr::FileAttributes;

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

impl FileAttributes {
    pub fn as_exfat_attr(&self) -> u16 {
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
        if self.dir {
            attr |= ExFatAttributes::DIRECTORY;
        }
        if self.archive {
            attr |= ExFatAttributes::ARCHIVE;
        }
        attr.bits()
    }

    pub fn from_exfat_attr(attr: u16) -> Self {
        let exfat_attr = ExFatAttributes::from_bits_truncate(attr);
        FileAttributes {
            read_only: exfat_attr.contains(ExFatAttributes::READ_ONLY),
            hidden: exfat_attr.contains(ExFatAttributes::HIDDEN),
            system: exfat_attr.contains(ExFatAttributes::SYSTEM),
            dir: exfat_attr.contains(ExFatAttributes::DIRECTORY),
            archive: exfat_attr.contains(ExFatAttributes::ARCHIVE),
            ..Default::default()
        }
    }
}
