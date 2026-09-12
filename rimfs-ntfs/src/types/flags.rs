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
        /// Record is part of the $Extend namespace
        const IN_EXTEND = 0x0004;
        /// Record owns view indexes such as $Secure:$SDH/$SII
        const IS_VIEW_INDEX = 0x0008;
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

/// Header byte of an NTFS Data Run
///
/// Encodes the length field size in the lower 4 bits and
/// the offset field size in the upper 4 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct DataRunHeader(pub u8);

impl DataRunHeader {
    /// Creates a new header from length size and offset size in bytes.
    #[inline]
    pub const fn new(length_size: u8, offset_size: u8) -> Self {
        Self((length_size & 0x0F) | ((offset_size & 0x0F) << 4))
    }

    /// Number of bytes used to store run length (0..15).
    #[inline]
    pub const fn length_size(self) -> usize {
        (self.0 & 0x0F) as usize
    }

    /// Number of bytes used to store cluster offset (0..15).
    #[inline]
    pub const fn offset_size(self) -> usize {
        ((self.0 >> 4) & 0x0F) as usize
    }

    /// Returns `true` if this is a sparse run (offset size is 0).
    #[inline]
    pub const fn is_sparse(self) -> bool {
        self.offset_size() == 0
    }

    /// Returns the raw header byte.
    #[inline]
    pub const fn raw(self) -> u8 {
        self.0
    }
}

impl From<u8> for DataRunHeader {
    #[inline]
    fn from(byte: u8) -> Self {
        Self(byte)
    }
}

impl From<DataRunHeader> for u8 {
    #[inline]
    fn from(header: DataRunHeader) -> Self {
        header.0
    }
}
