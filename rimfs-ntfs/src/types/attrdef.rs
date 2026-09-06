// SPDX-License-Identifier: MIT
//! NTFS Attribute Definitions ($AttrDef)
//!
//! Standard on-disk table defining NTFS attribute types, flags and size constraints.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{Immutable, IntoBytes};

/// Attribute Definition entry (160 bytes)
#[derive(Debug, Clone, Copy, IntoBytes, Immutable)]
#[repr(C, packed)]
pub struct AttributeDefinition {
    /// Name of the attribute (Unicode, zero padded)
    pub name: [u16; 64],
    /// Attribute type code (e.g., 0x10 for $STANDARD_INFORMATION)
    pub attr_type: u32,
    /// Display rule
    pub display_rule: u32,
    /// Collation rule
    pub collation_rule: u32,
    /// Flags
    pub flags: u32,
    /// Minimum size in bytes
    pub min_size: u64,
    /// Maximum size in bytes
    pub max_size: u64,
}

impl AttributeDefinition {
    pub fn new(name: &str, attr_type: u32, flags: u32, min_size: u64, max_size: u64) -> Self {
        let mut name_buf = [0u16; 64];
        for (i, c) in name.encode_utf16().take(64).enumerate() {
            name_buf[i] = c;
        }

        Self {
            name: name_buf,
            attr_type,
            display_rule: 0,
            collation_rule: 0,
            flags,
            min_size,
            max_size,
        }
    }
}

/// Build the standard set of attribute definitions (2560 bytes = 16 entries * 160 bytes).
///
/// Windows 11 and mkfs.ntfs require 15 valid definitions followed by a 160-byte null terminator.
pub fn build_standard_attr_defs() -> Vec<u8> {
    let defs = [
        AttributeDefinition::new("$STANDARD_INFORMATION", 0x10, 0x40, 48, 72),
        AttributeDefinition::new("$ATTRIBUTE_LIST", 0x20, 0x80, 0, u64::MAX),
        AttributeDefinition::new("$FILE_NAME", 0x30, 0x42, 68, 578),
        AttributeDefinition::new("$OBJECT_ID", 0x40, 0x40, 0, 256),
        AttributeDefinition::new("$SECURITY_DESCRIPTOR", 0x50, 0x80, 0, u64::MAX),
        AttributeDefinition::new("$VOLUME_NAME", 0x60, 0x40, 2, 256),
        AttributeDefinition::new("$VOLUME_INFORMATION", 0x70, 0x40, 12, 12),
        AttributeDefinition::new("$DATA", 0x80, 0x00, 0, u64::MAX),
        AttributeDefinition::new("$INDEX_ROOT", 0x90, 0x40, 0, u64::MAX),
        AttributeDefinition::new("$INDEX_ALLOCATION", 0xA0, 0x80, 0, u64::MAX),
        AttributeDefinition::new("$BITMAP", 0xB0, 0x80, 0, u64::MAX),
        AttributeDefinition::new("$REPARSE_POINT", 0xC0, 0x80, 0, 16384),
        AttributeDefinition::new("$EA_INFORMATION", 0xD0, 0x40, 8, 8),
        AttributeDefinition::new("$EA", 0xE0, 0x00, 0, 65536),
        AttributeDefinition::new("$LOGGED_UTILITY_STREAM", 0x100, 0x80, 0, 65536),
    ];

    let mut result = Vec::with_capacity(16 * 160);
    for def in defs {
        result.extend_from_slice(def.as_bytes());
    }
    // 16th entry is a null terminator (160 bytes of zero) required by Windows
    result.extend_from_slice(&[0u8; 160]);
    result
}
