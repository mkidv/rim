// SPDX-License-Identifier: MIT
//! NTFS Attribute Definitions ($AttrDef)

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
    pub fn new(name: &str, attr_type: u32, min_size: u64, max_size: u64) -> Self {
        let mut name_buf = [0u16; 64];
        for (i, c) in name.encode_utf16().take(64).enumerate() {
            name_buf[i] = c;
        }

        Self {
            name: name_buf,
            attr_type,
            display_rule: 0,
            collation_rule: 0,
            flags: 0,
            min_size,
            max_size,
        }
    }
}

/// Build the standard set of attribute definitions
pub fn build_standard_attr_defs() -> Vec<u8> {
    let defs = vec![
        AttributeDefinition::new("$STANDARD_INFORMATION", 0x10, 48, 72),
        AttributeDefinition::new("$ATTRIBUTE_LIST", 0x20, 0, 0xFFFFFFFF),
        AttributeDefinition::new("$FILE_NAME", 0x30, 68, 578),
        AttributeDefinition::new("$OBJECT_ID", 0x40, 0, 256),
        AttributeDefinition::new("$SECURITY_DESCRIPTOR", 0x50, 0, 0xFFFFFFFF),
        AttributeDefinition::new("$VOLUME_NAME", 0x60, 2, 256),
        AttributeDefinition::new("$VOLUME_INFORMATION", 0x70, 12, 12),
        AttributeDefinition::new("$DATA", 0x80, 0, 0xFFFFFFFF),
        AttributeDefinition::new("$INDEX_ROOT", 0x90, 0, 0xFFFFFFFF),
        AttributeDefinition::new("$INDEX_ALLOCATION", 0xA0, 0, 0xFFFFFFFF),
        AttributeDefinition::new("$BITMAP", 0xB0, 0, 0xFFFFFFFF),
        AttributeDefinition::new("$REPARSE_POINT", 0xC0, 0, 16384),
        AttributeDefinition::new("$EA_INFORMATION", 0xD0, 8, 8),
        AttributeDefinition::new("$EA", 0xE0, 0, 65536),
        AttributeDefinition::new("$LOGGED_UTILITY_STREAM", 0x100, 0, 65536),
    ];

    let mut result = Vec::with_capacity(defs.len() * 160);
    for def in defs {
        result.extend_from_slice(def.as_bytes());
    }
    result
}
