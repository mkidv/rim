// SPDX-License-Identifier: MIT

use crate::errors::{GenResult, LayoutError};
use crate::layout::constants::*;
use crate::layout::*;
use rimpart::{gpt::GptEntry, guids::*};

/// Encode a Partition as a GptEntry.
pub fn partition_to_gpt_entry(partition: &Partition<'_>, start: u64, end: u64) -> GptEntry {
    let type_guid = gpt_type_guid_for_kind(&partition.kind);
    GptEntry::new(
        type_guid,
        partition.guid,
        start,
        end,
        if partition.bootable { 1 } else { 0 },
        &partition.name,
    )
}

/// Encode a PartitionConfig as a GptEntry.
pub fn partition_config_to_gpt_entry(
    partition: &PartitionConfig,
    start: u64,
    end: u64,
) -> GenResult<GptEntry> {
    let type_guid = gpt_type_guid_for_kind(&partition.effective_kind());

    let unique_guid = if let Some(guid) = partition.guid {
        guid.as_u128().to_le_bytes()
    } else {
        [0u8; 16]
    };

    Ok(GptEntry::new(
        type_guid,
        unique_guid,
        start,
        end,
        if partition.bootable { 1 } else { 0 },
        &partition.name,
    ))
}

/// Map PartitionKind to GPT type GUID.
pub fn gpt_type_guid_for_kind(kind: &PartitionKind) -> [u8; 16] {
    match kind {
        PartitionKind::Esp => GPT_PARTITION_TYPE_ESP,
        PartitionKind::Data => GPT_PARTITION_TYPE_DATA,
        PartitionKind::Linux => GPT_PARTITION_TYPE_LINUX,
        PartitionKind::Biosboot => GPT_PARTITION_TYPE_BIOSBOOT,
        PartitionKind::Swap => GPT_PARTITION_TYPE_SWAP,
        PartitionKind::Boot => GPT_PARTITION_TYPE_BOOT,
        PartitionKind::Recovery => GPT_PARTITION_TYPE_RECOVERY,
    }
}

/// Convert Size to number of sectors.
pub fn size_to_sectors(size: &Size) -> u64 {
    match size {
        Size::Fixed(mib) => (mib * 1024 * 1024) / DEFAULT_SECTOR_SIZE,
        Size::Auto => unreachable!("Size::Auto must be resolved before conversion"),
    }
}

/// Calculate total disk sectors needed for a layout config.
pub fn calculate_total_disk_sectors_from_config(layout: &LayoutConfig) -> u64 {
    if let Some(Size::Fixed(mb)) = layout.disk.as_ref().and_then(|d| d.size.as_ref()) {
        return (mb * 1024 * 1024) / DEFAULT_SECTOR_SIZE;
    }
    layout
        .partitions
        .iter()
        .map(|p| size_to_sectors(&p.size) + DEFAULT_ALIGNMENT)
        .sum::<u64>()
        + DEFAULT_ALIGNMENT
}

/// Calculate total disk sectors needed for a layout.
pub fn calculate_total_disk_sectors(layout: &Layout<'_>) -> u64 {
    if let Some(total) = layout.total_disk_sectors {
        return total;
    }
    layout
        .partitions
        .iter()
        .map(|p| p.size_sectors + layout.alignment_sectors)
        .sum::<u64>()
        + layout.alignment_sectors
}

/// Parse alignment string to sector count.
pub fn parse_alignment_sectors(s: &str) -> GenResult<u64> {
    let lower = s.trim().to_lowercase();
    let bytes = if let Some(stripped) = lower.strip_suffix("k") {
        stripped
            .trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidAlignment {
                bytes: 0,
                sector_size: DEFAULT_SECTOR_SIZE,
            })?
            * 1024
    } else if let Some(stripped) = lower.strip_suffix("m") {
        stripped
            .trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidAlignment {
                bytes: 0,
                sector_size: DEFAULT_SECTOR_SIZE,
            })?
            * 1024
            * 1024
    } else if let Some(stripped) = lower.strip_suffix("g") {
        stripped
            .trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidAlignment {
                bytes: 0,
                sector_size: DEFAULT_SECTOR_SIZE,
            })?
            * 1024
            * 1024
            * 1024
    } else {
        s.trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidAlignment {
                bytes: 0,
                sector_size: DEFAULT_SECTOR_SIZE,
            })?
    };

    crate::ensure!(
        bytes % DEFAULT_SECTOR_SIZE == 0,
        LayoutError::InvalidAlignment {
            bytes,
            sector_size: DEFAULT_SECTOR_SIZE,
        }
    );

    Ok(bytes / DEFAULT_SECTOR_SIZE)
}
