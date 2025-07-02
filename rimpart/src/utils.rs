// SPDX-License-Identifier: MIT

use crate::{DEFAULT_SECTOR_SIZE, error::*, gpt, mbr, types::GptPartitionEntry};
use rimio::prelude::*;

/// Report struct for truncate_image (optional, allows nice reporting)
#[derive(Debug, Clone, Copy)]
pub struct TruncateReport {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub saved_bytes: u64,
}

/// Truncate the disk image to the last used sector of partitions.
///0
/// Works with any BlockIO (File, RAM, UEFI, ...).
///
/// Returns Ok(Some(TruncateReport)) if truncated, Ok(None) if no partitions.
pub fn truncate_image_custom_sector(
    io: &mut dyn BlockIOSetLen,
    partitions: &[GptPartitionEntry],
    total_sectors: u64,
    sector_size: u64,
) -> PartResult<Option<TruncateReport>> {
    // Compute last used sector
    if let Some(max_end_lba) = partitions.iter().map(|p| p.ending_lba).max() {
        let used_sectors = max_end_lba + 1;
        let used_bytes = used_sectors * sector_size;
        let total_bytes = total_sectors * sector_size;

        let saved_bytes = total_bytes.saturating_sub(used_bytes);

        io.set_len(used_bytes)?;

        Ok(Some(TruncateReport {
            total_bytes,
            used_bytes,
            saved_bytes,
        }))
    } else {
        // No partitions
        Ok(None)
    }
}

pub fn truncate_image(
    io: &mut dyn BlockIOSetLen,
    partitions: &[GptPartitionEntry],
    total_sectors: u64,
) -> PartResult<Option<TruncateReport>> {
    truncate_image_custom_sector(io, partitions, total_sectors, DEFAULT_SECTOR_SIZE)
}

pub fn detect_partition_offset_by_type_guid(
    io: &mut dyn BlockIO,
    type_guid: &[u8; 16],
) -> PartResult<u64> {
    // Step 1: Validate MBR has protective GPT
    let mbr = mbr::parse_mbr(io)?;
    if mbr.partition_entries[0].partition_type != 0xEE {
        return Err(PartError::Other("No GPT detected (missing 0xEE MBR entry)"));
    }

    // Step 2: Parse GPT
    let (_header, partitions) = gpt::parse_gpt(io)?;

    // Step 3: Find first matching partition
    let part = partitions
        .iter()
        .find(|p| p.partition_type_guid == *type_guid)
        .ok_or(PartError::Other("Matching partition not found"))?;

    Ok(part.starting_lba * DEFAULT_SECTOR_SIZE)
}

pub fn validate_full_disk(io: &mut dyn BlockIO) -> PartResult<()> {
    let (header, parts) = gpt::parse_gpt(io)?;

    let mbr = mbr::parse_mbr(io)?;
    mbr::validate_protective_mbr(&mbr, header.backup_lba + 1)?;

    gpt::validate_gpt_header(&header)?;
    gpt::validate_partition_bounds(&header, &parts)?;
    gpt::validate_partition_crc(&parts, header.partition_entries_crc32)?;
    Ok(())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_image_empty() {
        let mut buf = [0u8; 512 * 100];
        let mut io = MemBlockIO::new(&mut buf);
        let result = truncate_image(&mut io, &[], 100).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_detect_partition_offset_fail() {
        let mut buf = [0u8; 512 * 100];
        let mut io = MemBlockIO::new(&mut buf);
        assert!(detect_partition_offset_by_type_guid(&mut io, &[0u8; 16]).is_err());
    }
}
