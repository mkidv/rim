// SPDX-License-Identifier: MIT

use crate::errors::{GenResult, LayoutError};
use crate::layout::constants::*;
use crate::layout::*;
use rimpart::{gpt::GptEntry, guids::*};

/// Encode a Partition as a GPTPartitionEntry.
pub fn partition_to_gpt_partition_entry(
    partition: &Partition,
    start: u64,
    end: u64,
) -> GenResult<GptEntry> {
    let type_guid = gpt_type_guid_for_kind(&partition.effective_kind());

    let unique_guid = partition
        .guid
        .unwrap_or_else(uuid::Uuid::new_v4)
        .as_u128()
        .to_le_bytes();

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

/// Calculate total disk sectors needed for a layout.
pub fn calculate_total_disk_sectors(layout: &Layout) -> u64 {
    layout
        .partitions
        .iter()
        .map(|p| size_to_sectors(&p.size) + DEFAULT_ALIGNMENT)
        .sum::<u64>()
        + DEFAULT_ALIGNMENT
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

    if bytes % DEFAULT_SECTOR_SIZE != 0 {
        return Err(LayoutError::InvalidAlignment {
            bytes,
            sector_size: DEFAULT_SECTOR_SIZE,
        }
        .into());
    }

    Ok(bytes / DEFAULT_SECTOR_SIZE)
}
