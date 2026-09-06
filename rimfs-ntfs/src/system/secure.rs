// SPDX-License-Identifier: MIT
//! Logic for building $Secure system file content
//!
//! $Secure contains:
//! - $SDS (Security Descriptor Stream): A stream of self-relative security descriptors.
//! - $SII (Security Id Index): Maps SecurityId -> Offset in $SDS.
//! - $SDH (Security Descriptor Hash Index): Maps (Hash, SecurityId) -> Offset in $SDS.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::{
    constant::{SECURITY_ID_EVERYONE, SECURITY_ID_SYSTEM},
    types::{
        IndexEntryHeader,
        security::{SecurityDescriptorHeader, SecurityHashKey, SecurityIdKey, SecurityIndexData},
        security_descriptor_everyone, security_descriptor_system,
    },
};
use zerocopy::IntoBytes;

/// Content for the $Secure file attributes
pub struct SecureFileContent {
    /// Content of the $SDS data stream
    pub sds: Vec<u8>,
    /// Content of the $SII index root (entries + end marker)
    pub sii_entries: Vec<u8>,
    /// Content of the $SDH index root (entries + end marker)
    pub sdh_entries: Vec<u8>,
}

/// Calculates the hash for a security descriptor as expected by NTFS.
///
/// The algorithm is a rotating sum of 32-bit words.
pub fn calculate_security_hash(data: &[u8]) -> u32 {
    let mut hash = 0u32;
    for i in (0..data.len()).step_by(4) {
        let mut word = 0u32;
        for j in 0..4 {
            if i + j < data.len() {
                word |= (data[i + j] as u32) << (j * 8);
            }
        }
        hash = hash.rotate_left(3).wrapping_add(word);
    }
    hash
}

/// Generates the default $Secure file content with "Everyone: Full Control" descriptor.
pub fn build_secure_content() -> SecureFileContent {
    let descriptors = [
        (SECURITY_ID_EVERYONE, security_descriptor_everyone()),
        (SECURITY_ID_SYSTEM, security_descriptor_system()),
    ];

    let mut sds = Vec::new();

    let mut sii_items = Vec::new();
    let mut sdh_items = Vec::new();

    for (security_id, payload) in descriptors {
        let hash = calculate_security_hash(&payload);

        // Each SDS entry begins on a 16-byte boundary.
        let offset = (sds.len() as u64 + 15) & !15;

        if sds.len() < offset as usize {
            sds.resize(offset as usize, 0);
        }

        let header_len = core::mem::size_of::<SecurityDescriptorHeader>() as u32;
        let raw_len = header_len + payload.len() as u32;
        let aligned_len = (raw_len + 15) & !15;

        let header = SecurityDescriptorHeader {
            hash,
            security_id,
            offset,
            length: raw_len,
        };

        sds.extend_from_slice(header.as_bytes());
        sds.extend_from_slice(&payload);
        sds.resize(offset as usize + aligned_len as usize, 0);

        let data = SecurityIndexData {
            hash,
            security_id,
            offset,
            length: raw_len,
        };

        sii_items.push((SecurityIdKey { security_id }, data));

        sdh_items.push((SecurityHashKey { hash, security_id }, data));
    }

    sdh_items.sort_by_key(|(k, _)| (k.hash, k.security_id));
    sii_items.sort_by_key(|(k, _)| k.security_id);

    // Primary SDS stream length (aligned to 16 bytes)
    let primary_len = sds.len();

    // The $SDS stream must be structured into 256 KiB blocks with a mirror copy
    // at offset 256 KiB (0x40000) as required by Windows ntfs.sys.
    const SDS_BLOCK_SIZE: usize = 256 * 1024;
    sds.resize(SDS_BLOCK_SIZE, 0);
    let primary_copy = sds[..primary_len].to_vec();
    sds.extend_from_slice(&primary_copy);

    SecureFileContent {
        sds,
        sii_entries: build_sii_root_content_multi(&sii_items),
        sdh_entries: build_sdh_root_content_multi(&sdh_items),
    }
}

fn build_sii_root_content_multi(entries: &[(SecurityIdKey, SecurityIndexData)]) -> Vec<u8> {
    let mut buf = Vec::new();

    for (key, data) in entries {
        buf.extend_from_slice(&0x14u16.to_le_bytes());
        buf.extend_from_slice(&0x14u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0x28u16.to_le_bytes());
        buf.extend_from_slice(&0x04u16.to_le_bytes());
        buf.push(0);
        buf.extend_from_slice(&[0u8; 3]);

        buf.extend_from_slice(key.as_bytes());
        buf.extend_from_slice(data.as_bytes());
    }

    buf.extend_from_slice(IndexEntryHeader::end_marker().as_bytes());
    buf
}

fn build_sdh_root_content_multi(entries: &[(SecurityHashKey, SecurityIndexData)]) -> Vec<u8> {
    let mut buf = Vec::new();

    for (key, data) in entries {
        buf.extend_from_slice(&0x18u16.to_le_bytes());
        buf.extend_from_slice(&0x14u16.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0x30u16.to_le_bytes());
        buf.extend_from_slice(&0x08u16.to_le_bytes());
        buf.push(0);
        buf.extend_from_slice(&[0u8; 3]);

        buf.extend_from_slice(key.as_bytes());
        buf.extend_from_slice(data.as_bytes());

        // Canonical NTFS SDH padding.
        buf.extend_from_slice(b"I\0I\0");
    }

    buf.extend_from_slice(IndexEntryHeader::end_marker().as_bytes());
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sii_layout_compliance() {
        let hash = 0x12345678;
        let id = 256;
        let offset = 0x1000;
        let length = 0x40;

        let key = SecurityIdKey { security_id: id };
        let data = SecurityIndexData {
            hash,
            security_id: id,
            offset,
            length,
        };

        let res = build_sii_root_content_multi(&[(key, data)]);

        // Spec: Entry size 0x28 (40 bytes), Key size 0x04 (4 bytes), Data size 0x14 (20 bytes).
        // Total buffer should be 40 (Entry) + 16 (End Marker) = 56 bytes.
        assert_eq!(res.len(), 40 + 16);

        // Header Check
        assert_eq!(&res[0..2], &0x14u16.to_le_bytes()); // OffsetToData 20
        assert_eq!(&res[2..4], &0x14u16.to_le_bytes()); // SizeOfData 20
        assert_eq!(&res[4..8], &[0; 4]); // Padding
        assert_eq!(&res[8..10], &0x28u16.to_le_bytes()); // Length 40
        assert_eq!(&res[10..12], &0x04u16.to_le_bytes()); // KeyLength 4

        // Key Check (offset 16)
        assert_eq!(&res[16..20], &id.to_le_bytes());

        // Data Check (offset 20)
        assert_eq!(&res[20..24], &hash.to_le_bytes());
        assert_eq!(&res[24..28], &id.to_le_bytes());
        assert_eq!(&res[28..36], &offset.to_le_bytes());
        assert_eq!(&res[36..40], &length.to_le_bytes());

        // End Marker Check
        assert_eq!(&res[40..48], &[0; 8]);
        assert_eq!(&res[48..50], &16u16.to_le_bytes());
        assert_eq!(res[52], 0x02); // LAST_ENTRY
    }

    #[test]
    fn test_sdh_layout_compliance() {
        let hash = 0xAABBCCDD;
        let id = 256;
        let offset = 0x2000;
        let length = 0x60;

        let key = SecurityHashKey {
            hash,
            security_id: id,
        };
        let data = SecurityIndexData {
            hash,
            security_id: id,
            offset,
            length,
        };

        let res = build_sdh_root_content_multi(&[(key, data)]);

        // Spec: Entry size 0x30 (48 bytes), Key size 0x08 (8 bytes), Data size 0x14 (20 bytes).
        // Includes "II" padding.
        // Total buffer should be 48 (Entry) + 16 (End Marker) = 64 bytes.
        assert_eq!(res.len(), 48 + 16);

        // Header Check
        assert_eq!(&res[0..2], &0x18u16.to_le_bytes()); // OffsetToData 24
        assert_eq!(&res[2..4], &0x14u16.to_le_bytes()); // SizeOfData 20
        assert_eq!(&res[4..8], &[0; 4]); // Padding
        assert_eq!(&res[8..10], &0x30u16.to_le_bytes()); // Length 48
        assert_eq!(&res[10..12], &0x08u16.to_le_bytes()); // KeyLength 8

        // Key Check (offset 16)
        assert_eq!(&res[16..20], &hash.to_le_bytes());
        assert_eq!(&res[20..24], &id.to_le_bytes());

        // Data Check (offset 24)
        assert_eq!(&res[24..28], &hash.to_le_bytes());
        assert_eq!(&res[28..32], &id.to_le_bytes());
        assert_eq!(&res[32..40], &offset.to_le_bytes());
        assert_eq!(&res[40..44], &length.to_le_bytes());

        // Padding "II" Check (offset 44)
        assert_eq!(&res[44..48], b"I\0I\0");

        // End Marker Check
        assert_eq!(res[60], 0x02); // LAST_ENTRY
    }
}
