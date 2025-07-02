// SPDX-License-Identifier: MIT
// rimgen/fs/ext4/attr.rs

use crate::core::parser::attr::FileAttributes;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct Ext4Mode: u16 {
        // File type
        const SOCKET  = 0xC000;
        const SYMLINK = 0xA000;
        const REGULAR = 0x8000;
        const BLOCK   = 0x6000;
        const DIR     = 0x4000;
        const CHARDEV = 0x2000;
        const FIFO    = 0x1000;

        // Owner permissions
        const OWNER_R = 0x0100;
        const OWNER_W = 0x0080;
        const OWNER_X = 0x0040;

        // Group permissions
        const GROUP_R = 0x0020;
        const GROUP_W = 0x0010;
        const GROUP_X = 0x0008;

        // Others permissions
        const OTHER_R = 0x0004;
        const OTHER_W = 0x0002;
        const OTHER_X = 0x0001;
    }
}

impl FileAttributes {
    pub fn as_ext4_file_type(&self) -> u8 {
        if self.dir {
            2 // directory
        } else {
            1 // regular file
        }
    }
    
    pub fn as_ext4_mode(&self) -> Ext4Mode {
        // Determine type
        let type_bits = if self.dir {
            Ext4Mode::DIR
        } else {
            Ext4Mode::REGULAR
        };

        // Determine permissions
        let perms = self
            .mode
            .map(|mode| {
                let mut bits = Ext4Mode::empty();

                if mode & 0o400 != 0 {
                    bits |= Ext4Mode::OWNER_R;
                }
                if mode & 0o200 != 0 {
                    bits |= Ext4Mode::OWNER_W;
                }
                if mode & 0o100 != 0 {
                    bits |= Ext4Mode::OWNER_X;
                }

                if mode & 0o040 != 0 {
                    bits |= Ext4Mode::GROUP_R;
                }
                if mode & 0o020 != 0 {
                    bits |= Ext4Mode::GROUP_W;
                }
                if mode & 0o010 != 0 {
                    bits |= Ext4Mode::GROUP_X;
                }

                if mode & 0o004 != 0 {
                    bits |= Ext4Mode::OTHER_R;
                }
                if mode & 0o002 != 0 {
                    bits |= Ext4Mode::OTHER_W;
                }
                if mode & 0o001 != 0 {
                    bits |= Ext4Mode::OTHER_X;
                }

                bits
            })
            .unwrap_or_else(|| {
                // Default perms: 0755 for dir, 0644 for file
                if self.dir {
                    Ext4Mode::OWNER_R
                        | Ext4Mode::OWNER_W
                        | Ext4Mode::OWNER_X
                        | Ext4Mode::GROUP_R
                        | Ext4Mode::GROUP_X
                        | Ext4Mode::OTHER_R
                        | Ext4Mode::OTHER_X
                } else {
                    Ext4Mode::OWNER_R | Ext4Mode::OWNER_W | Ext4Mode::GROUP_R | Ext4Mode::OTHER_R
                }
            });

        type_bits | perms
    }
}
