// SPDX-License-Identifier: MIT
//! NTFS Security-related structures
//!
//! Reference: [MS-DTYP]: Windows Data Types
//! <https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-dtyp/f993ad91-88f2-4bd5-a131-7e8c0db1683d>

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

/// Security Descriptor Control flags
pub const SE_SELF_RELATIVE: u16 = 0x8000;
pub const SE_DACL_PRESENT: u16 = 0x0004;

/// Security Descriptor (Self-Relative)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct SecurityDescriptorRelative {
    pub revision: u8,
    pub sbz1: u8,
    pub control: u16,
    pub owner_offset: u32,
    pub group_offset: u32,
    pub sacl_offset: u32,
    pub dacl_offset: u32,
}

/// ACL Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct AclHeader {
    pub revision: u8,
    pub sbz1: u8,
    pub acl_size: u16,
    pub ace_count: u16,
    pub sbz2: u16,
}

/// ACE Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct AceHeader {
    pub ace_type: u8,
    pub ace_flags: u8,
    pub ace_size: u16,
}

pub const ACCESS_ALLOWED_ACE_TYPE: u8 = 0x00;

/// Access Allowed ACE
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct AccessAllowedAce {
    pub header: AceHeader,
    pub mask: u32,
    // SID follows
}

/// SID Header (fixed part)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct SidHeader {
    pub revision: u8,
    pub sub_authority_count: u8,
    pub identifier_authority: [u8; 6],
}

/// Header for an entry in the $SDS (Security Descriptor Stream)
///
/// This header precedes the actual self-relative security descriptor in the $SDS stream.
/// It MUST be 16-byte aligned within the stream.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct SecurityDescriptorHeader {
    /// Hash of the security descriptor
    pub hash: u32,
    /// Unique Security ID
    pub security_id: u32,
    /// Offset of this entry in the $SDS stream
    pub offset: u64,
    /// Size of this entry (Header + SD + Padding)
    pub length: u32,
}

/// Data for $SII (Security Id Index) and $SDH (Security Descriptor Hash)
///
/// Both $SII and $SDH indexes use this structure as the data part of their index entries.
/// It mimics the SecurityDescriptorHeader but is stored in the index.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct SecurityIndexData {
    pub hash: u32,
    pub security_id: u32,
    pub offset: u64,
    pub length: u32,
}

/// Key for the $SII (Security Id Index)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct SecurityIdKey {
    pub security_id: u32,
}

/// Key for the $SDH (Security Descriptor Hash Index)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct SecurityHashKey {
    pub hash: u32,
    pub security_id: u32,
}

/// Create a default "Everyone: Full Control" security descriptor
pub fn security_descriptor_everyone() -> Vec<u8> {
    vec![
        0x01, 0x00, 0x04, 0x80, // Revision, SBZ, Control (SE_DACL_PRESENT | SE_SELF_RELATIVE)
        0x14, 0x00, 0x00, 0x00, // Owner offset (20)
        0x24, 0x00, 0x00, 0x00, // Group offset (36)
        0x00, 0x00, 0x00, 0x00, // SACL offset
        0x34, 0x00, 0x00, 0x00, // DACL offset (52)
        // Owner: S-1-5-32-544 (Administrators)
        0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x20, 0x02, 0x00,
        0x00, // Group: S-1-5-32-544 (Administrators)
        0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x20, 0x02, 0x00,
        0x00, // DACL
        0x02, 0x00, 0x1C, 0x00, // ACL Header (Rev 2, Size 28)
        0x01, 0x00, 0x00, 0x00, // ACE Count 1
        // ACE 1 (Allow Everyone Full Control)
        0x00, 0x00, 0x14, 0x00, // Type 0 (Allow), Flags 0, Size 20
        0xFF, 0x01, 0x1F, 0x00, // Mask 0x001F01FF (Full Control)
        // SID (S-1-1-0 "Everyone")
        0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ]
}

/// Create a canonical "Boring" security descriptor
/// Owner = SYSTEM (S-1-5-18)
/// Group = SYSTEM (S-1-5-18)
/// DACL = Everyone : Full Control
pub fn security_descriptor_boring() -> Vec<u8> {
    vec![
        0x01, 0x00, 0x04, 0x80, // Revision, SBZ, Control (SE_DACL_PRESENT | SE_SELF_RELATIVE)
        0x14, 0x00, 0x00, 0x00, // Owner offset (20)
        0x20, 0x00, 0x00, 0x00, // Group offset (32)
        0x00, 0x00, 0x00, 0x00, // SACL offset
        0x2C, 0x00, 0x00, 0x00, // DACL offset (44)
        // Owner: S-1-5-18 (SYSTEM)
        0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00,
        // Group: S-1-5-18 (SYSTEM)
        0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00, // DACL
        0x02, 0x00, 0x1C, 0x00, // ACL Header (Rev 2, Size 28)
        0x01, 0x00, 0x00, 0x00, // ACE Count 1 (DACL)
        // ACE 1 (Allow Everyone Full Control)
        0x00, 0x00, 0x14, 0x00, // Type 0 (Allow), Flags 0, Size 20
        0xFF, 0x01, 0x1F, 0x00, // Mask 0x001F01FF (Full Control)
        // SID (S-1-1-0 "Everyone")
        0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    ]
}
