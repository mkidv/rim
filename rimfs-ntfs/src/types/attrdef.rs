// SPDX-License-Identifier: MIT
//! NTFS Attribute Definitions ($AttrDef)
//!
//! Standard on-disk table defining NTFS attribute types, flags and size constraints.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Attribute Definition entry (160 bytes)
#[derive(Debug, Clone, Copy, FromBytes, KnownLayout, IntoBytes, Immutable)]
#[repr(C)]
pub struct AttributeDefinition {
    /// Name of the attribute (Unicode, zero padded)
    pub name: [U16; 64],
    /// Attribute type code (e.g., 0x10 for $STANDARD_INFORMATION)
    pub attr_type: U32,
    /// Display rule
    pub display_rule: U32,
    /// Collation rule
    pub collation_rule: U32,
    /// Flags
    pub flags: U32,
    /// Minimum size in bytes
    pub min_size: U64,
    /// Maximum size in bytes
    pub max_size: U64,
}

impl AttributeDefinition {
    pub fn new(name: &str, attr_type: u32, flags: u32, min_size: u64, max_size: u64) -> Self {
        let mut name_buf = [U16::new(0); 64];
        for (i, c) in name.encode_utf16().take(64).enumerate() {
            name_buf[i] = c.into();
        }

        Self {
            name: name_buf,
            attr_type: attr_type.into(),
            display_rule: 0.into(),
            collation_rule: 0.into(),
            flags: flags.into(),
            min_size: min_size.into(),
            max_size: max_size.into(),
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

const _: () = {
    assert!(core::mem::size_of::<AttributeDefinition>() == 160);
    assert!(core::mem::align_of::<AttributeDefinition>() == 1);
    assert!(core::mem::offset_of!(AttributeDefinition, attr_type) == 128);
    assert!(core::mem::offset_of!(AttributeDefinition, min_size) == 144);
};
