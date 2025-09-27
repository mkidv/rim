// SPDX-License-Identifier: MIT

use crate::layout::constants::*;
use crate::layout::*;

use anyhow::Result;
use rimpart::{gpt::GptEntry, guids::*};

/// Encode a Partition as a GPTPartitionEntry.
pub fn partition_to_gpt_partition_entry(
    partition: &Partition,
    start: u64,
    end: u64,
) -> Result<GptEntry> {
    let type_guid = gpt_type_guid_for_kind(&partition.effective_kind());
    
    let unique_guid = partition
        .guid
        .ok_or_else(|| anyhow::anyhow!("Missing GUID for '{}'", partition.name))?
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
        Size::Fixed(mib) => (mib * 1024 * 1024) / SECTOR_SIZE,
        Size::Auto => unreachable!("Size::Auto must be resolved before conversion"),
    }
}

/// Convert Size to number of bytes.
pub fn size_to_bytes(size: &Size) -> u64 {
    match size {
        Size::Fixed(mib) => mib * 1024 * 1024,
        Size::Auto => unreachable!("Size::Auto must be resolved before conversion"),
    }
}
