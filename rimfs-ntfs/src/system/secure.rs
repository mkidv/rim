// SPDX-License-Identifier: MIT
//! Logic for building $Secure system file content
//!
//! $Secure contains:
//! - $SDS (Security Descriptor Stream): A stream of self-relative security descriptors.
//! - $SII (Security Id Index): Maps SecurityId -> Offset in $SDS.
//! - $SDH (Security Descriptor Hash Index): Maps (Hash, SecurityId) -> Offset in $SDS.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::constant::SECURITY_ID_EVERYONE;
use crate::types::security::{
    SecurityDescriptorHeader, SecurityHashKey, SecurityIdKey, SecurityIndexData,
    security_descriptor_boring,
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
    // 1. Generate Security Descriptor (SD)
    let sd_payload = security_descriptor_boring();
    let sd_len = sd_payload.len() as u32;

    // 2. Build $SDS Stream
    // Header (24 bytes) + SD + Padding
    let header_len = core::mem::size_of::<SecurityDescriptorHeader>() as u32;
    let raw_len = header_len + sd_len;
    // Align total entry size to 16 bytes
    let aligned_len = (raw_len + 15) & !15;

    // Calculate proper hash
    let hash = calculate_security_hash(&sd_payload);
    let offset = 0u64; // First entry at offset 0

    let sds_header = SecurityDescriptorHeader {
        hash,
        security_id: SECURITY_ID_EVERYONE,
        offset, // Relative to start of stream
        length: aligned_len,
    };

    let mut sds = Vec::with_capacity(aligned_len as usize);
    sds.extend_from_slice(sds_header.as_bytes());
    sds.extend_from_slice(&sd_payload);
    // Add padding
    sds.resize(aligned_len as usize, 0);

    // 3. Build $SII Index Entries
    // Key: SecurityId
    // Data: SecurityIndexData
    let sii_key = SecurityIdKey {
        security_id: SECURITY_ID_EVERYONE,
    };
    let index_data = SecurityIndexData {
        hash,
        security_id: SECURITY_ID_EVERYONE,
        offset,
        length: aligned_len,
    };

    // 4. Build $SDH Index Entries
    // Key: Hash + SecurityId
    // Data: SecurityIndexData
    let sdh_key = SecurityHashKey {
        hash,
        security_id: SECURITY_ID_EVERYONE,
    };

    let sdh_entries = build_sdh_root_content(&sdh_key, &index_data);

    SecureFileContent {
        sds,
        sii_entries: build_sii_root_content(&sii_key, &index_data),
        sdh_entries,
    }
}

/// Helper to build $SII (Security Id Index) root content.
/// Spec: Entry size 0x28 (40 bytes), Key size 0x04 (4 bytes), Data size 0x14 (20 bytes).
fn build_sii_root_content(key: &SecurityIdKey, data: &SecurityIndexData) -> Vec<u8> {
    let mut buf = Vec::new();

    // --- Entry 1 ---
    // Header (16 bytes)
    // Offset 0x00 (4): OffsetToData (2 bits offset, 2 bits size? No, it's 2+2 u16)
    // Spec says: 0x00 (2) Offset to data, 0x02 (2) Size of data, 0x04 (4) Padding 0
    buf.extend_from_slice(&0x14u16.to_le_bytes()); // Offset to data (16 header + 4 key = 20)
    buf.extend_from_slice(&0x14u16.to_le_bytes()); // Size of data (20)
    buf.extend_from_slice(&0u32.to_le_bytes()); // Padding
    buf.extend_from_slice(&0x28u16.to_le_bytes()); // Length = 40
    buf.extend_from_slice(&0x04u16.to_le_bytes()); // KeyLength = 4
    buf.push(0); // Flags
    buf.extend_from_slice(&[0u8; 3]); // Padding

    // Key (4 bytes)
    buf.extend_from_slice(key.as_bytes());
    // Data (20 bytes)
    buf.extend_from_slice(data.as_bytes());

    // --- End Entry ---
    buf.extend_from_slice(&0u64.to_le_bytes()); // FileRef
    buf.extend_from_slice(&16u16.to_le_bytes()); // Length
    buf.extend_from_slice(&0u16.to_le_bytes()); // KeyLength
    buf.push(0x02); // Flags = LAST_ENTRY
    buf.extend_from_slice(&[0u8; 3]);

    buf
}

/// Helper to build $SDH (Security Descriptor Hash Index) root content.
/// Spec: Entry size 0x30 (48 bytes), Key size 0x08 (8 bytes), Data size 0x14 (20 bytes).
/// Includes trailing "II" padding (4 bytes).
fn build_sdh_root_content(key: &SecurityHashKey, data: &SecurityIndexData) -> Vec<u8> {
    let mut buf = Vec::new();

    // --- Entry 1 ---
    // Header (16 bytes)
    // Spec: 0x00 (2) Offset to data, 0x02 (2) Size of data, 0x04 (4) Padding 0
    buf.extend_from_slice(&0x18u16.to_le_bytes()); // Offset to data (16 header + 8 key = 24)
    buf.extend_from_slice(&0x14u16.to_le_bytes()); // Size of data (20)
    buf.extend_from_slice(&0u32.to_le_bytes()); // Padding
    buf.extend_from_slice(&0x30u16.to_le_bytes()); // Length = 48
    buf.extend_from_slice(&0x08u16.to_le_bytes()); // KeyLength = 8
    buf.push(0); // Flags
    buf.extend_from_slice(&[0u8; 3]); // Padding

    // Key (8 bytes)
    buf.extend_from_slice(key.as_bytes());
    // Data (20 bytes)
    buf.extend_from_slice(data.as_bytes());

    // Padding (4 bytes): Unicode "II"
    buf.extend_from_slice(b"I\0I\0");

    // --- End Entry ---
    buf.extend_from_slice(&0u64.to_le_bytes()); // FileRef
    buf.extend_from_slice(&16u16.to_le_bytes()); // Length
    buf.extend_from_slice(&0u16.to_le_bytes()); // KeyLength
    buf.push(0x02); // Flags = LAST_ENTRY
    buf.extend_from_slice(&[0u8; 3]);

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

        let res = build_sii_root_content(&key, &data);

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

        let res = build_sdh_root_content(&key, &data);

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
