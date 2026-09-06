// SPDX-License-Identifier: MIT
//! NTFS Collation Rules for Index Root
//!
//! Reference: Microsoft NTFS documentation & ntfs-3g

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u32)]
pub enum NtfsCollationRule {
    /// Collation by binary comparison (memcmp)
    Binary = 0x00,
    /// Collation for File Names (case-insensitive via $UpCase, then case-sensitive)
    FileName = 0x01,
    /// Collation for Unicode strings
    Unicode = 0x02,
    /// Collation for 32-bit unsigned integers (used by $Quota:$Q)
    Ulong = 0x10,
    /// Collation for Security Identifiers (used by $Quota:$O)
    Sid = 0x11,
    /// Collation for Security Hashes (used by $Secure:$SDH)
    SecurityHash = 0x12,
    /// Collation for multiple 32-bit unsigned integers (used by $ObjId, $Reparse, $Secure:$SII)
    Ulongs = 0x13,
}

impl NtfsCollationRule {
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self as u32
    }

    pub const fn from_u32(val: u32) -> Option<Self> {
        match val {
            0x00 => Some(Self::Binary),
            0x01 => Some(Self::FileName),
            0x02 => Some(Self::Unicode),
            0x10 => Some(Self::Ulong),
            0x11 => Some(Self::Sid),
            0x12 => Some(Self::SecurityHash),
            0x13 => Some(Self::Ulongs),
            _ => None,
        }
    }
}

impl From<NtfsCollationRule> for u32 {
    #[inline]
    fn from(rule: NtfsCollationRule) -> Self {
        rule.as_u32()
    }
}
