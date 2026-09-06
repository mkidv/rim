// SPDX-License-Identifier: MIT

use crate::core::traits::{FileAttributes, NodeKind};
use crate::types::ExtFileType;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ExtMode: u16 {
        // File type
        const SOCKET  = 0xC000;
        const SYMLINK = 0xA000;
        const REGULAR = 0x8000;
        const BLOCK   = 0x6000;
        const DIR     = 0x4000;
        const CHARDEV = 0x2000;
        const FIFO    = 0x1000;

        // Special execution flags
        const SETUID  = 0x0800; // 0o4000
        const SETGID  = 0x0400; // 0o2000
        const STICKY  = 0x0200; // 0o1000

        // Owner permissions
        const OWNER_R = 0x0100; // 0o0400
        const OWNER_W = 0x0080; // 0o0200
        const OWNER_X = 0x0040; // 0o0100

        // Group permissions
        const GROUP_R = 0x0020; // 0o0040
        const GROUP_W = 0x0010; // 0o0020
        const GROUP_X = 0x0008; // 0o0010

        // Others permissions
        const OTHER_R = 0x0004; // 0o0004
        const OTHER_W = 0x0002; // 0o0002
        const OTHER_X = 0x0001; // 0o0001
    }
}

pub trait ExtFileAttributesExt {
    fn as_ext4_file_type(&self) -> u8;
    fn as_ext4_mode(&self) -> ExtMode;
}

impl ExtFileAttributesExt for FileAttributes {
    fn as_ext4_file_type(&self) -> u8 {
        ExtFileType::from(self.kind).as_u8()
    }

    fn as_ext4_mode(&self) -> ExtMode {
        // Determine type bits
        let type_bits = match self.kind {
            NodeKind::Directory => ExtMode::DIR,
            NodeKind::Symlink => ExtMode::SYMLINK,
            NodeKind::Regular => ExtMode::REGULAR,
            NodeKind::CharDevice => ExtMode::CHARDEV,
            NodeKind::BlockDevice => ExtMode::BLOCK,
            NodeKind::Fifo => ExtMode::FIFO,
            NodeKind::Socket => ExtMode::SOCKET,
        };

        // Determine permissions (including special bits: setuid 0o4000, setgid 0o2000, sticky 0o1000)
        let perms = self
            .mode
            .map(|mode| {
                // Keep 12 bits: special bits (0o7000) + permission bits (0o0777) = 0o7777 (0x0FFF)
                ExtMode::from_bits_truncate((mode & 0x0FFF) as u16)
            })
            .unwrap_or_else(|| {
                // Default perms: 0755 for dir, 0777 for symlink, 0644 for regular file
                match self.kind {
                    NodeKind::Directory => {
                        ExtMode::OWNER_R
                            | ExtMode::OWNER_W
                            | ExtMode::OWNER_X
                            | ExtMode::GROUP_R
                            | ExtMode::GROUP_X
                            | ExtMode::OTHER_R
                            | ExtMode::OTHER_X
                    }
                    NodeKind::Symlink => {
                        ExtMode::OWNER_R
                            | ExtMode::OWNER_W
                            | ExtMode::OWNER_X
                            | ExtMode::GROUP_R
                            | ExtMode::GROUP_W
                            | ExtMode::GROUP_X
                            | ExtMode::OTHER_R
                            | ExtMode::OTHER_W
                            | ExtMode::OTHER_X
                    }
                    _ => ExtMode::OWNER_R | ExtMode::OWNER_W | ExtMode::GROUP_R | ExtMode::OTHER_R,
                }
            });

        type_bits | perms
    }
}
