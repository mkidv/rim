use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ProtectiveMbrEntry {
    pub boot_indicator: u8,
    pub starting_chs: [u8; 3],
    pub partition_type: u8,
    pub ending_chs: [u8; 3],
    pub starting_lba: u32,
    pub size_in_lba: u32,
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ProtectiveMbr {
    pub bootstrap_code: [u8; 446],
    pub partition_entries: [ProtectiveMbrEntry; 4],
    pub signature: [u8; 2],
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct GptHeader {
    pub signature: [u8; 8],
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub reserved: u32,
    pub current_lba: u64,
    pub backup_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub num_partition_entries: u32,
    pub partition_entry_size: u32,
    pub partition_entries_crc32: u32,
    pub reserved2: [u8; 420],
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct GptPartitionEntry {
    pub partition_type_guid: [u8; 16],
    pub unique_partition_guid: [u8; 16],
    pub starting_lba: u64,
    pub ending_lba: u64,
    pub attributes: u64,
    pub partition_name: [u16; 36],
}
