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
    let counter = COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

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
    let counter = COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed) as u128;

    let mut id = (seconds << 64) | (millis << 32) | counter;

    // Mix 1 LSB of the counter into byte 0
    id ^= counter & 0xFF;

    id
}

/// Computes the FAT size and cluster count for a given FAT configuration.
///
/// This function performs convergence to determine the optimal FAT size (`fat_size`)
/// and the number of clusters (`cluster_count`) based on the FAT file system parameters.
///
/// # Arguments
/// - `sector_size`: Size of a sector in bytes (e.g., 512)
/// - `total_sectors`: Total number of sectors on the volume
/// - `reserved_sectors`: Number of reserved sectors (before the FAT area)
/// - `entry_size`: Size of a FAT entry (in bytes, e.g., 4 for FAT32)
/// - `min_entries`: Minimum number of FAT entries (often 2 for FAT12/16/32)
/// - `fat_count`: Number of FAT copies (usually 2)
/// - `sectors_per_cluster`: Number of sectors per cluster
///
/// # Returns
/// Tuple `(fat_size, cluster_count)`
/// - `fat_size`: FAT size in sectors
/// - `cluster_count`: Number of data clusters
pub fn converge_fat_layout(
    sector_size: u32,
    total_sectors: u32,
    reserved_sectors: u32,
    entry_size: u32,
    min_entries: u32,
    num_fats: u8,
    sectors_per_cluster: u32,
) -> (u32, u32) {
    let mut cluster_count = 0;
    let mut fat_size = 0;
    let mut prev_fat_size = fat_size;
    loop {
        let entries = cluster_count + min_entries;
        fat_size = (entries * entry_size).div_ceil(sector_size); // assumes 512 here
        let fat_area = fat_size * num_fats as u32;
        let data_sectors = total_sectors - reserved_sectors - fat_area;
        let new_cluster_count = data_sectors / sectors_per_cluster;

        if new_cluster_count == cluster_count && fat_size == prev_fat_size {
            break;
        }

        cluster_count = new_cluster_count;
        prev_fat_size = fat_size;
    }

    (fat_size, cluster_count)
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
