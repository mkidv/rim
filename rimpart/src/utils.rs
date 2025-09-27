// SPDX-License-Identifier: MIT

use crate::{DEFAULT_SECTOR_SIZE, errors::*, gpt, mbr};
use rimio::prelude::*;

/// Report struct for truncate_image (optional, allows nice reporting)
#[derive(Debug, Clone, Copy)]
pub struct TruncateReport {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub saved_bytes: u64,
}

/// Truncate the disk image to the last used sector of partitions.
/// Works with any BlockIO (File, RAM, UEFI, ...).
///
/// Returns Ok(Some(TruncateReport)) if truncated, Ok(None) if no partitions.
pub fn truncate_image_custom_sector(
    io: &mut dyn BlockIOSetLen,
    partitions: &[gpt::GptEntry],
    total_sectors: u64,
    sector_size: u64,
) -> PartResult<Option<TruncateReport>> {
    // Compute last used sector
    if let Some(max_end_lba) = partitions.iter().map(|p| p.end_lba).max() {
        let used_sectors = max_end_lba + 1;
        let used_bytes = used_sectors.saturating_mul(sector_size);
        let total_bytes = total_sectors.saturating_mul(sector_size);

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
    partitions: &[gpt::GptEntry],
    total_sectors: u64,
) -> PartResult<Option<TruncateReport>> {
    truncate_image_custom_sector(io, partitions, total_sectors, DEFAULT_SECTOR_SIZE)
}

/// Detect the byte offset of the first partition that matches `type_guid`.
/// - Verifies MBR is protective (0xEE)
/// - Parses GPT and finds the first matching entry
pub fn detect_partition_offset_by_type_guid(
    io: &mut dyn BlockIO,
    type_guid: &[u8; 16],
) -> PartResult<u64> {
    detect_partition_offset_by_type_guid_with_sector(io, type_guid, DEFAULT_SECTOR_SIZE)
}

pub fn detect_partition_offset_by_type_guid_with_sector(
    io: &mut dyn BlockIO,
    type_guid: &[u8; 16],
    sector_size: u64,
) -> PartResult<u64> {
    let m = mbr::read_mbr(io)?;
    m.validate_protective(0)?;
    let (_h, parts) = gpt::read_gpt_with_sector(io, sector_size)?;
    let part = parts
        .iter()
        .find(|p| p.type_guid == *type_guid)
        .ok_or(PartError::Other("Matching partition not found"))?;
    Ok(part.start_lba.saturating_mul(sector_size))
}

/// Full-disk validation:
/// - Parse & validate GPT (header + entries + overlaps/alignment déjà faits)
/// - Parse MBR & validate protective entry coherent with disk size
pub fn validate_full_disk(io: &mut dyn BlockIO) -> PartResult<()> {
    // GPT (valide header + entries + overlaps/alignments via parse_gpt)
    let (header, _parts) = gpt::read_gpt(io)?;

    // Cohérence MBR protectif avec la taille disque
    let total_sectors = header.backup_lba + 1;
    let m = mbr::read_mbr(io)?;
    m.validate_protective(total_sectors)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_image_empty() {
        let mut buf = [0u8; 512 * 100];
        let mut io = MemBlockIO::new(&mut buf);
        let result = truncate_image(&mut io, &[], 100).unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn detect_partition_offset_fail() {
        let mut buf = [0u8; 512 * 100];
        let mut io = MemBlockIO::new(&mut buf);
        assert!(detect_partition_offset_by_type_guid(&mut io, &[0u8; 16]).is_err());
    }
}
