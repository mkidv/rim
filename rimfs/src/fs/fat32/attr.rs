// SPDX-License-Identifier: MIT

use crate::core::parser::*;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct Fat32Attributes: u8 {
        const READ_ONLY = 0x01;
        const HIDDEN    = 0x02;
        const SYSTEM    = 0x04;
        const VOLUME_ID = 0x08;
        const DIRECTORY = 0x10;
        const ARCHIVE   = 0x20;
        const LFN       = 0x0F;
    }
}

impl FileAttributes {
    pub fn as_fat_attr(&self) -> u8 {
        let mut attr = Fat32Attributes::empty();
        if self.read_only {
            attr |= Fat32Attributes::READ_ONLY;
        }
        if self.hidden {
            attr |= Fat32Attributes::HIDDEN;
        }
        if self.system {
            attr |= Fat32Attributes::SYSTEM;
        }
        if self.dir {
            attr |= Fat32Attributes::DIRECTORY;
        }
        if self.archive {
            attr |= Fat32Attributes::ARCHIVE;
        }
        attr.bits()
    }

    pub fn from_fat_attr(attr: u8) -> Self {
        let fat_attr = Fat32Attributes::from_bits_truncate(attr);
        FileAttributes {
            read_only: fat_attr.contains(Fat32Attributes::READ_ONLY),
            hidden: fat_attr.contains(Fat32Attributes::HIDDEN),
            system: fat_attr.contains(Fat32Attributes::SYSTEM),
            dir: fat_attr.contains(Fat32Attributes::DIRECTORY),
            archive: fat_attr.contains(Fat32Attributes::ARCHIVE),
            ..Default::default()
        }
    }
}
