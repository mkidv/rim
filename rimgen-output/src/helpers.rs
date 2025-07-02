// SPDX-License-Identifier: MIT
// rimgen-output/src/gpt.rs

use crate::constants::*;
use rimgen_layout::*;

use anyhow::Result;
use rimpart::{guids::*, types::GptPartitionEntry};

/// Encode a Partition as a GPTPartitionEntry.
pub fn partition_to_gpt_partition_entry(
    partition: &Partition,
    start: u64,
    end: u64,
) -> Result<GptPartitionEntry> {
    // Filtrer les caractères ASCII, max 35 caractères
    let safe_name: String = partition
        .name
        .chars()
        .filter(|c| c.is_ascii())
        .take(35)
        .collect();

    // Encoder en UTF-16, on veut 36 u16 (terminé par 0 si plus court)
    let mut name_utf16 = [0u16; 36];
    for (i, c) in safe_name.encode_utf16().take(36).enumerate() {
        name_utf16[i] = c;
    }

    // Construire l'entrée GPT
    Ok(GptPartitionEntry {
        partition_type_guid: gpt_type_guid_for_kind(&partition.effective_kind()),
        unique_partition_guid: partition
            .guid
            .ok_or_else(|| anyhow::anyhow!("Missing GUID for '{}'", partition.name))?
            .as_u128()
            .to_le_bytes(),
        starting_lba: start,
        ending_lba: end,
        attributes: if partition.bootable { 1 } else { 0 },
        partition_name: name_utf16,
    })
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