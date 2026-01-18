// SPDX-License-Identifier: MIT

//! Volume utils.
//!
//! Provides utilities for generating volume identifiers for file systems such as FAT32 and EXT4,
//! as well as a function to converge the FAT layout (computing FAT size and cluster count).
//!
//! - Uses the current UTC time for ID generation.
//! - Works in `no_std` using `UNIX_EPOCH` if needed.
//!
//! Functions:
//! - `generate_volume_id_4()` → 4-byte ID (FAT32 style)
//! - `generate_volume_id_16()` → 16-byte ID (EXT4 style)
//! - `converge_fat_layout()` → Computes FAT size and cluster count for a given FAT configuration.

use crate::core::utils::time_utils;
static COUNTER: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Generates a 4-byte volume identifier, suitable for FAT32.
///
/// Combines the current timestamp (seconds + milliseconds) into 4 bytes.
/// In `no_std`, uses `UNIX_EPOCH` if needed.
///
/// Not guaranteed to be globally unique, but sufficient for typical FAT32 usage.
pub fn generate_volume_id_32() -> u32 {
    let now = time_utils::now_utc();

    let seconds = now.unix_timestamp() as u32;
    let millis = now.millisecond() as u32;
    let counter = COUNTER.fetch_add(5, core::sync::atomic::Ordering::Relaxed);

    let mut id = (seconds & 0xFFFF) | ((millis & 0xFF) << 16) | ((millis >> 8) << 24);

    // Mix 1 LSB of the counter into byte 0
    id ^= counter & 0xFF;

    id
}

/// Generates a 16-byte volume identifier, suitable for EXT4.
///
/// Combines the current timestamp into a 128-bit value.
/// In `no_std`, uses `UNIX_EPOCH` if needed.
///
/// Can be used as an EXT4 volume UUID or for other purposes.
pub fn generate_volume_id_128() -> u128 {
    let now = time_utils::now_utc();

    let seconds = now.unix_timestamp() as u128;
    let millis = now.millisecond() as u128;
    let micros = now.microsecond() as u128;
    let counter = COUNTER.fetch_add(5, core::sync::atomic::Ordering::Relaxed) as u128;

    let mut id =
        (seconds & 0xFFFF) | (seconds << 64) | ((millis & 0xFF) << 16) | ((micros >> 8) << 32);

    // Mix 1 LSB of the counter into byte 0
    id ^= counter & 0xFF;

    id
}

#[inline]
fn crc32_ieee(mut crc: u32, bytes: &[u8]) -> u32 {
    const P: u32 = 0xEDB88320;
    crc ^= 0xFFFFFFFF;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg() & P;
            crc = (crc >> 1) ^ mask;
        }
    }
    !crc
}

#[inline]
fn xorshift32(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Seed = concat(label upper, size_bytes LE, cluster_size LE, user_salt LE)
pub fn derive_ids(
    label: &str,
    size_bytes: u64,
    cluster_size: u32,
    user_salt: u32,
) -> ([u8; 16], u32) {
    let mut tmp = [0u8; 8 + 4 + 64];
    let mut n = 0;

    for b in label.bytes().take(32) {
        tmp[n] = b.to_ascii_uppercase();
        n += 1;
    }
    tmp[n..n + 8].copy_from_slice(&size_bytes.to_le_bytes());
    n += 8;
    tmp[n..n + 4].copy_from_slice(&cluster_size.to_le_bytes());
    n += 4;
    tmp[n..n + 4].copy_from_slice(&user_salt.to_le_bytes());
    n += 4;

    let seed = crc32_ieee(0, &tmp[..n]);

    let mut x = xorshift32(seed ^ 0x9E37_79B9);
    let mut guid = [0u8; 16];
    for i in 0..4 {
        x = xorshift32(x);
        guid[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    guid[6] = (guid[6] & 0x0F) | 0x40;
    guid[8] = (guid[8] & 0x3F) | 0x80;

    let vol_id = crc32_ieee(0, &guid);

    (guid, vol_id)
}

#[inline]
pub fn guid_from_volume_id(seed: u32) -> [u8; 16] {
    let mut x = seed ^ 0x9E37_79B9;
    let mut out = [0u8; 16];
    for i in 0..4 {
        x = xorshift32(x);
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    out[6] = (out[6] & 0x0F) | 0x40;
    out[8] = (out[8] & 0x3F) | 0x80;
    out
}

#[inline]
pub fn volume_id_from_guid(guid: &[u8; 16]) -> u32 {
    crc32_ieee(0, guid)
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_generate_volume_id_32() {
        let id1 = generate_volume_id_32();
        let id2 = generate_volume_id_32();

        println!("Volume ID u32 {id1:02X?} {id2:02X?}");

        // Check correct length
        assert_eq!(id1.to_le_bytes().len(), 4);
        assert_eq!(id2.to_le_bytes().len(), 4);

        // Optionally: allow same result if very fast, but in test we check they differ
        assert_ne!(id1, id2, "Two volume IDs should not be equal");
    }

    #[test]
    fn test_generate_volume_id_128() {
        let id1 = generate_volume_id_128();
        let id2 = generate_volume_id_128();

        println!("Volume ID u128: {id1:02X?} {id2:02X?}");

        assert_eq!(id1.to_le_bytes().len(), 16);
        assert_eq!(id2.to_le_bytes().len(), 16);

        assert_ne!(id1, id2, "Two volume IDs should not be equal");
    }
}
