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
        const I30_INDEX = 0x1000_0000;
        const VIEW_INDEX = 0x2000_0000;
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

bitflags::bitflags! {
    /// Windows NT File Access Rights (ACCESS_MASK)
    ///
    /// Reference: [MS-DTYP] 2.4.3 ACCESS_MASK
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct FileAccessMask: u32 {
        // Specific file permissions
        const FILE_READ_DATA        = 0x0000_0001; // file read / dir list
        const FILE_WRITE_DATA       = 0x0000_0002; // file write / dir create file
        const FILE_APPEND_DATA      = 0x0000_0004; // file append / dir create subdir
        const FILE_READ_EA          = 0x0000_0008;
        const FILE_WRITE_EA         = 0x0000_0010;
        const FILE_EXECUTE          = 0x0000_0020; // file execute / dir traverse
        const FILE_DELETE_CHILD     = 0x0000_0040;
        const FILE_READ_ATTRIBUTES  = 0x0000_0080;
        const FILE_WRITE_ATTRIBUTES = 0x0000_0100;

        // Standard rights
        const DELETE                = 0x0001_0000;
        const READ_CONTROL          = 0x0002_0000;
        const WRITE_DAC             = 0x0004_0000;
        const WRITE_OWNER           = 0x0008_0000;
        const SYNCHRONIZE           = 0x0010_0000;

        // Standard combinations
        const STANDARD_RIGHTS_REQUIRED = 0x000F_0000;
        const STANDARD_RIGHTS_READ     = 0x0002_0000;
        const STANDARD_RIGHTS_WRITE    = 0x0002_0000;
        const STANDARD_RIGHTS_EXECUTE  = 0x0002_0000;
        const STANDARD_RIGHTS_ALL      = 0x001F_0000;

        // Generic / Composite rights used by NTFS
        const FULL_CONTROL   = 0x001F_01FF;
        const MODIFY         = 0x0013_01BF;
        const READ_AND_EXEC  = 0x0012_00A9;
        const READ_EXEC_DIR  = 0x0012_0089;
        const SYSTEM_CONTROL = 0x0012_019F;
    }
}

bitflags::bitflags! {
    /// ACE (Access Control Entry) Header Flags
    ///
    /// Reference: [MS-DTYP] 2.4.4.1 ACE_HEADER
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct AceFlags: u8 {
        /// Non-container child objects inherit the ACE
        const OBJECT_INHERIT        = 0x01;
        /// Child containers inherit the ACE
        const CONTAINER_INHERIT     = 0x02;
        /// Does not propagate to subsequent generations of containers
        const NO_PROPAGATE_INHERIT  = 0x04;
        /// Applies only to child objects, not to the object itself
        const INHERIT_ONLY          = 0x08;
        /// The ACE was inherited from a parent object
        const INHERITED             = 0x10;
        /// Generates audit messages for successful access
        const SUCCESSFUL_ACCESS     = 0x40;
        /// Generates audit messages for failed access
        const FAILED_ACCESS         = 0x80;
    }
}

bitflags::bitflags! {
    /// Security Descriptor Control Flags
    ///
    /// Reference: [MS-DTYP] 2.4.6 SECURITY_DESCRIPTOR
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct SecurityDescriptorControl: u16 {
        const OWNER_DEFAULTED       = 0x0001;
        const GROUP_DEFAULTED       = 0x0002;
        const DACL_PRESENT          = 0x0004;
        const DACL_DEFAULTED        = 0x0008;
        const SACL_PRESENT          = 0x0010;
        const SACL_DEFAULTED        = 0x0020;
        const DACL_AUTO_INHERIT_REQ = 0x0100;
        const SACL_AUTO_INHERIT_REQ = 0x0200;
        const DACL_AUTO_INHERITED   = 0x0400;
        const SACL_AUTO_INHERITED   = 0x0800;
        const DACL_PROTECTED        = 0x1000;
        const SACL_PROTECTED        = 0x2000;
        const RM_CONTROL_VALID      = 0x4000;
        const SELF_RELATIVE         = 0x8000;
    }
}

bitflags::bitflags! {
    /// NTFS Quota Control Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct QuotaFlags: u32 {
        const DEFAULT_LIMITS       = 0x0000_0001;
        const LIMITS_OUT_OF_DATE   = 0x0000_0002;
        const LOG_THRESHOLD        = 0x0000_0004;
        const LOG_LIMIT            = 0x0000_0008;
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
    fn from(hdr: DataRunHeader) -> Self {
        hdr.0
    }
}
