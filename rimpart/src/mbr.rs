use crate::{
    error::*,
    types::{ProtectiveMbr, ProtectiveMbrEntry},
};
use rimio::prelude::*;

pub fn write_protective_mbr<IO: BlockIO + ?Sized>(io: &mut IO, total_sectors: u64) -> PartResult {
    let mut partition_entries = [ProtectiveMbrEntry {
        boot_indicator: 0x00,
        starting_chs: [0x00, 0x02, 0x00],
        partition_type: 0xEE,
        ending_chs: [0xFE, 0xFF, 0xFF],
        starting_lba: 1,
        size_in_lba: if total_sectors > u32::MAX as u64 {
            u32::MAX
        } else {
            total_sectors as u32
        },
    }; 4];

    for entry in partition_entries[1..].iter_mut() {
        *entry = ProtectiveMbrEntry {
            boot_indicator: 0x00,
            starting_chs: [0, 0, 0],
            partition_type: 0x00,
            ending_chs: [0, 0, 0],
            starting_lba: 0,
            size_in_lba: 0,
        };
    }

    let mbr = ProtectiveMbr {
        bootstrap_code: [0u8; 446],
        partition_entries,
        signature: [0x55, 0xAA],
    };

    io.write_struct(0, &mbr)?;

    io.flush()?;

    Ok(())
}

pub fn parse_mbr<IO: BlockIO + ?Sized>(io: &mut IO) -> PartResult<ProtectiveMbr> {
    let mbr: ProtectiveMbr = io.read_struct(0)?;
    validate_protective_mbr(&mbr, 0)?;
    Ok(mbr)
}

/// Validates that the MBR is a valid protective MBR (0xEE at partition[0].type, signature OK).
///
/// You may optionally pass the `total_sectors` to check if it matches `size_in_lba` (when available).
pub fn validate_protective_mbr(mbr: &ProtectiveMbr, total_sectors: u64) -> PartResult<()> {
    if mbr.signature != [0x55, 0xAA] {
        return Err(PartError::Invalid("Invalid signature"));
    }

    let entry = &mbr.partition_entries[0];
    if entry.partition_type != 0xEE {
        return Err(PartError::Other(
            "MBR does not contain protective GPT entry (0xEE)",
        ));
    }

    // Optional sector bounds validation
    if total_sectors > 0 {
        if total_sectors > u32::MAX as u64 && entry.size_in_lba != u32::MAX {
            return Err(PartError::Other(
                "Invalid protective MBR size (should be 0xFFFF_FFFF)",
            ));
        } else if total_sectors <= u32::MAX as u64 && entry.size_in_lba != total_sectors as u32 {
            return Err(PartError::Other(
                "Protective MBR size does not match disk size",
            ));
        }
    }

    Ok(())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    #[test]
    fn test_write_and_parse_protective_mbr() {
        let mut buf = [0u8; 512];
        let mut io = MemBlockIO::new(&mut buf);

        write_protective_mbr(&mut io, 2048).unwrap();
        let mbr = parse_mbr(&mut io).unwrap();

        assert_eq!(mbr.signature, [0x55, 0xAA]);
        assert_eq!(mbr.partition_entries[0].partition_type, 0xEE);
    }

    #[test]
    fn test_validate_mbr_invalid_signature() {
        let mbr = ProtectiveMbr {
            bootstrap_code: [0; 446],
            partition_entries: [ProtectiveMbrEntry {
                boot_indicator: 0,
                starting_chs: [0, 0, 0],
                partition_type: 0xEE,
                ending_chs: [0, 0, 0],
                starting_lba: 1,
                size_in_lba: 1,
            }; 4],
            signature: [0x00, 0x00],
        };

        assert!(validate_protective_mbr(&mbr, 1).is_err());
    }
}
