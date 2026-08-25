// SPDX-License-Identifier: MIT
//! NTFS flags and attributes

bitflags::bitflags! {
    /// MFT Record Header Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct MftRecordFlags: u16 {
        /// Record is in use
        const IN_USE = 0x0001;
        /// Record is a directory
        const IS_DIRECTORY = 0x0002;
    }
}

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
    }
}

bitflags::bitflags! {
    /// Attribute Header Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct AttributeFlags: u16 {
        const COMPRESSED = 0x0001;
        const ENCRYPTED  = 0x4000;
        const SPARSE     = 0x8000;
    }
}

bitflags::bitflags! {
    /// Volume Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct NtfsVolumeFlags: u16 {
        const DIRTY = 0x0001;
        const RESIZE_LOG_FILE = 0x0002;
        const UPGRADE_ON_MOUNT = 0x0004;
        const MOUNTED_ON_NT4 = 0x0008;
        const DELETE_USN_UNDERWAY = 0x0010;
        const REPAIR_OBJECT_ID = 0x0020;
        const CHKDSK_RUN_ONCE = 0x8000;
    }
}

bitflags::bitflags! {
    /// Index Node Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct IndexNodeFlags: u8 {
        /// Index node has children nodes
        const HAS_CHILDREN = 0x01;
    }
}

bitflags::bitflags! {
    /// Index Entry Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct IndexEntryFlags: u8 {
        /// Entry points to a sub-node
        const HAS_SUBNODES = 0x01;
        const LAST_ENTRY   = 0x02;
    }
}
