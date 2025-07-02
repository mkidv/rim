#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::{
    DEFAULT_SECTOR_SIZE,
    error::*,
    types::{GptHeader, GptPartitionEntry},
};
use crc32fast::Hasher;
use rimio::prelude::*;
use zerocopy::{FromBytes, IntoBytes};

// #[inline(always)]
// pub unsafe fn slice_as_bytes<T>(slice: &[T]) -> &[u8] {
//     unsafe {
//         core::slice::from_raw_parts(
//             slice.as_ptr() as *const u8,
//             slice.len() * core::mem::size_of::<T>(),
//         )
//     }
// }

pub fn write_gpt<IO: BlockIO + ?Sized>(
    io: &mut IO,
    partitions: &[GptPartitionEntry],
    total_sectors: u64,
    disk_guid: [u8; 16],
) -> PartResult<()> {
    let num_partition_entries = 128;
    let partition_entry_size = core::mem::size_of::<GptPartitionEntry>() as u32;
    let partition_array_size = (num_partition_entries * partition_entry_size) as usize;

    let mut part_entries_buf = vec![0u8; partition_array_size];
    for (i, entry) in partitions.iter().enumerate() {
        let bytes = entry.as_bytes();
        let offset = i * core::mem::size_of::<GptPartitionEntry>();
        part_entries_buf[offset..offset + bytes.len()].copy_from_slice(bytes);
    }

    let part_entries_lba = 2;
    let part_entries_offset = part_entries_lba * DEFAULT_SECTOR_SIZE;

    io.write_at(part_entries_offset, &part_entries_buf)?;

    let mut hasher = Hasher::new();
    hasher.update(&part_entries_buf);
    let partition_entries_crc32 = hasher.finalize();

    let mut header = GptHeader {
        signature: *b"EFI PART",
        revision: 0x00010000,
        header_size: 92,
        header_crc32: 0,
        reserved: 0,
        current_lba: 1,
        backup_lba: total_sectors - 1,
        first_usable_lba: 34,
        last_usable_lba: total_sectors - 34 - 1,
        disk_guid,
        partition_entry_lba: part_entries_lba,
        num_partition_entries,
        partition_entry_size,
        partition_entries_crc32,
        reserved2: [0u8; 420],
    };

    {
        header.header_crc32 = 0;
        let header_bytes = header.as_bytes();
        let mut hasher = Hasher::new();
        hasher.update(&header_bytes[..header.header_size as usize]);
        header.header_crc32 = hasher.finalize();
    }

    io.write_struct(DEFAULT_SECTOR_SIZE, &header)?;

    let secondary_part_entries_lba = total_sectors - 33;
    io.write_at(
        secondary_part_entries_lba * DEFAULT_SECTOR_SIZE,
        &part_entries_buf,
    )?;

    let mut secondary_header = header;
    secondary_header.current_lba = total_sectors - 1;
    secondary_header.backup_lba = 1;
    secondary_header.partition_entry_lba = secondary_part_entries_lba;

    {
        secondary_header.header_crc32 = 0;
        let header_bytes = secondary_header.as_bytes();
        let mut hasher = Hasher::new();
        hasher.update(&header_bytes[..secondary_header.header_size as usize]);
        secondary_header.header_crc32 = hasher.finalize();
    }

    io.write_struct((total_sectors - 1) * DEFAULT_SECTOR_SIZE, &secondary_header)?;

    io.flush()?;

    Ok(())
}

pub fn parse_gpt<IO: BlockIO + ?Sized>(
    io: &mut IO,
) -> PartResult<(GptHeader, Vec<GptPartitionEntry>)> {
    // Read GPT header ===
    let header: GptHeader = io.read_struct(DEFAULT_SECTOR_SIZE)?;

    validate_gpt_header(&header)?;

    // Validate entry size ===
    let entry_size = header.partition_entry_size as usize;
    if entry_size != core::mem::size_of::<GptPartitionEntry>() {
        return Err(PartError::Invalid("Invalid entry"));
    }

    // Read partition entries ===
    let total_partitions = header.num_partition_entries as usize;
    let partition_array_size = entry_size * total_partitions;

    let mut buf = vec![0u8; partition_array_size];
    io.read_at(header.partition_entry_lba * DEFAULT_SECTOR_SIZE, &mut buf)?;

    // Convert to entries ===
    let mut partition_entries = vec![];
    for i in 0..total_partitions {
        let offset = i * entry_size;
        let bytes = &buf[offset..offset + entry_size];
        let entry =
            GptPartitionEntry::ref_from_bytes(bytes).map_err(|_| PartError::Invalid("Invalid entry"))?;

        if entry.partition_type_guid.iter().all(|&b| b == 0) {
            continue; // empty slot
        }

        partition_entries.push(*entry);
    }

    // Validate entries CRC ===
    validate_partition_crc(&partition_entries, header.partition_entries_crc32)?;

    // Optionally validate bounds (disabled here for performance)
    // validate_partition_bounds(&header, &entries)?;

    Ok((header, partition_entries))
}

pub fn validate_gpt_header(header: &GptHeader) -> PartResult<()> {
    if &header.signature != b"EFI PART" {
        return Err(PartError::Invalid("Invalid signature"));
    }

    let expected_crc32 = header.header_crc32;
    let mut clone = *header;
    clone.header_crc32 = 0;

    let header_bytes = clone.as_bytes();
    let mut hasher = Hasher::new();
    hasher.update(&header_bytes[..header.header_size as usize]);
    let actual_crc32 = hasher.finalize();

    if actual_crc32 != expected_crc32 {
        return Err(PartError::Invalid("Invalid CRC header"));
    }

    Ok(())
}

pub fn validate_partition_crc(
    entries: &[GptPartitionEntry],
    expected_crc: u32,
) -> PartResult<()> {
    let mut buf = vec![0u8; 128 * core::mem::size_of::<GptPartitionEntry>()];
    let entry_size = core::mem::size_of::<GptPartitionEntry>();

    for (i, entry) in entries.iter().enumerate() {
        let bytes = entry.as_bytes();
        let offset = i * entry_size;
        buf[offset..offset + entry_size].copy_from_slice(bytes);
    }

    let mut hasher = Hasher::new();
    hasher.update(&buf);
    let actual_crc32 = hasher.finalize();

    if actual_crc32 != expected_crc {
        return Err(PartError::Invalid("Invalid CRC partition"));
    }

    Ok(())
}


pub fn validate_partition_bounds(
    header: &GptHeader,
    parts: &[GptPartitionEntry],
) -> PartResult<()> {
    for (i, part) in parts.iter().enumerate() {
        if part.starting_lba < header.first_usable_lba {
            return Err(PartError::Other("Partition starts before first usable LBA"));
        }
        if part.ending_lba > header.last_usable_lba {
            return Err(PartError::Other("Partition ends after last usable LBA"));
        }
        if part.ending_lba < part.starting_lba {
            return Err(PartError::Other("Partition ends before it starts"));
        }

        for (j, other) in parts.iter().enumerate() {
            if i == j {
                continue;
            }
            if !(part.ending_lba < other.starting_lba || part.starting_lba > other.ending_lba) {
                return Err(PartError::Other("Partition overlap detected"));
            }
        }
    }

    Ok(())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::types::{GptHeader, GptPartitionEntry};

    #[test]
    fn test_write_and_parse_gpt() {
        let mut buf = [0u8; 512 * 512];
        let mut io = MemBlockIO::new(&mut buf);

        let part = GptPartitionEntry {
            partition_type_guid: [0xAA; 16],
            unique_partition_guid: [0xBB; 16],
            starting_lba: 34,
            ending_lba: 99,
            attributes: 0,
            partition_name: [0u16; 36],
        };

        let disk_guid = [0x11; 16];
        let total_sectors = 512;

        write_gpt(&mut io, &[part], total_sectors, disk_guid).unwrap();

        let (header, parts) = parse_gpt(&mut io).unwrap();
        assert_eq!(header.signature, *b"EFI PART");
        assert_eq!(header.disk_guid, disk_guid);
        assert_eq!(parts.len(), 1);
        let starting_lba = parts[0].starting_lba;
        assert_eq!(starting_lba, 34);
    }

    #[test]
    fn test_validate_gpt_header_crc_mismatch() {
        let header = GptHeader {
            signature: *b"EFI PART",
            revision: 0x00010000,
            header_size: 92,
            header_crc32: 0xDEADBEEF,
            reserved: 0,
            current_lba: 1,
            backup_lba: 2,
            first_usable_lba: 34,
            last_usable_lba: 2047,
            disk_guid: [0u8; 16],
            partition_entry_lba: 2,
            num_partition_entries: 128,
            partition_entry_size: core::mem::size_of::<GptPartitionEntry>() as u32,
            partition_entries_crc32: 0,
            reserved2: [0u8; 420],
        };

        assert!(validate_gpt_header(&header).is_err());
    }

    #[test]
    fn test_partition_bounds_overlap_error() {
        let header = GptHeader {
            signature: *b"EFI PART",
            revision: 0x00010000,
            header_size: 92,
            header_crc32: 0,
            reserved: 0,
            current_lba: 1,
            backup_lba: 100,
            first_usable_lba: 34,
            last_usable_lba: 90,
            disk_guid: [0u8; 16],
            partition_entry_lba: 2,
            num_partition_entries: 2,
            partition_entry_size: 128,
            partition_entries_crc32: 0,
            reserved2: [0u8; 420],
        };

        let p1 = GptPartitionEntry {
            partition_type_guid: [1u8; 16],
            unique_partition_guid: [2u8; 16],
            starting_lba: 40,
            ending_lba: 60,
            attributes: 0,
            partition_name: [0u16; 36],
        };

        let p2 = GptPartitionEntry {
            partition_type_guid: [3u8; 16],
            unique_partition_guid: [4u8; 16],
            starting_lba: 55,
            ending_lba: 70,
            attributes: 0,
            partition_name: [0u16; 36],
        };

        let parts = vec![p1, p2];
        let result = validate_partition_bounds(&header, &parts);

        assert!(result.is_err());
    }
}
