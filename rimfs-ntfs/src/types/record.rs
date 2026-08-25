// SPDX-License-Identifier: MIT

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::constant::*;
use crate::flags::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AttributeType {
    StandardInformation = ATTR_STANDARD_INFORMATION,
    AttributeList = ATTR_ATTRIBUTE_LIST,
    FileName = ATTR_FILE_NAME,
    ObjectId = ATTR_OBJECT_ID,
    SecurityDescriptor = ATTR_SECURITY_DESCRIPTOR,
    VolumeName = ATTR_VOLUME_NAME,
    VolumeInformation = ATTR_VOLUME_INFORMATION,
    Data = ATTR_DATA,
    IndexRoot = ATTR_INDEX_ROOT,
    IndexAllocation = ATTR_INDEX_ALLOCATION,
    Bitmap = ATTR_BITMAP,
    ReparsePoint = ATTR_REPARSE_POINT,
    EaInformation = ATTR_EA_INFORMATION,
    Ea = ATTR_EA,
    LoggedUtilityStream = ATTR_LOGGED_UTILITY_STREAM,
    End = ATTR_END,
}

impl AttributeType {
    pub fn code(self) -> u32 {
        self as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NtfsFileNameNamespace {
    Posix = FILE_NAME_POSIX,
    Win32 = FILE_NAME_WIN32,
    Dos = FILE_NAME_DOS,
    Win32AndDos = FILE_NAME_WIN32_AND_DOS,
}

impl NtfsFileNameNamespace {
    pub fn bits(self) -> u8 {
        self as u8
    }
}

/// MFT Record Header (Multi_Sector_Header)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct MftRecordHeader {
    /// "FILE"
    pub signature: [u8; 4],
    /// Offset to Update Sequence Array
    pub usa_offset: u16,
    /// Size in words of Update Sequence Array
    pub usa_count: u16,
    /// $LogFile Sequence Number
    pub lsn: u64,
    /// Sequence Number (reused)
    pub sequence_number: u16,
    /// Reference Count (Hard links)
    pub link_count: u16,
    /// Offset to first Attribute
    pub attrs_offset: u16,
    /// Flags (IN_USE, DIRECTORY)
    pub flags: u16,
    /// Real size of the FILE record
    pub bytes_used: u32,
    /// Allocated size of the FILE record
    pub bytes_allocated: u32,
    /// File Reference to the Base FILE record
    pub base_file_record: u64,
    /// Next Attribute ID
    pub next_attr_id: u16,
    /// Unused / Padding
    pub reserved: u16,
    /// MFT Record Number (index)
    pub mft_record_number: u32,
}

impl MftRecordHeader {
    pub fn new(record_number: u32, flags: MftRecordFlags, allocated_size: u32) -> Self {
        Self {
            signature: *b"FILE",
            usa_offset: 48,
            usa_count: 0, // Set by formatter
            lsn: 0,
            sequence_number: 1, // Default, incremented on reuse
            link_count: 1,
            attrs_offset: 0, // Set by formatter
            flags: flags.bits(),
            bytes_used: 0, // Set by formatter
            bytes_allocated: allocated_size,
            base_file_record: 0,
            next_attr_id: 0,
            reserved: 0,
            mft_record_number: record_number,
        }
    }

    pub fn with_sequence_number(mut self, seq: u16) -> Self {
        self.sequence_number = seq;
        self
    }

    pub fn is_in_use(&self) -> bool {
        (self.flags & MftRecordFlags::IN_USE.bits()) != 0
    }

    pub fn is_file_record(&self) -> bool {
        &self.signature == b"FILE"
    }

    pub fn is_dir(&self) -> bool {
        (self.flags & MftRecordFlags::IS_DIRECTORY.bits()) != 0
    }

    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

/// Generic Attribute Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct AttributeHeader {
    pub attr_type: u32,
    pub length: u32,
    pub non_resident: u8,
    pub name_length: u8,
    pub name_offset: u16,
    pub flags: u16,
    pub attr_id: u16,
}

impl AttributeHeader {
    pub fn is_resident(&self) -> bool {
        self.non_resident == 0
    }

    pub fn is_non_resident(&self) -> bool {
        self.non_resident != 0
    }

    pub fn is_standard_information(&self) -> bool {
        self.attr_type == AttributeType::StandardInformation.code()
    }

    pub fn is_file_name(&self) -> bool {
        self.attr_type == AttributeType::FileName.code()
    }

    pub fn is_data(&self) -> bool {
        self.attr_type == AttributeType::Data.code()
    }

    pub fn is_index_root(&self) -> bool {
        self.attr_type == AttributeType::IndexRoot.code()
    }

    pub fn is_index_allocation(&self) -> bool {
        self.attr_type == AttributeType::IndexAllocation.code()
    }

    pub fn is_bitmap(&self) -> bool {
        self.attr_type == AttributeType::Bitmap.code()
    }

    pub fn is_reparse_point(&self) -> bool {
        self.attr_type == AttributeType::ReparsePoint.code()
    }

    pub fn is_ea_information(&self) -> bool {
        self.attr_type == AttributeType::EaInformation.code()
    }

    pub fn is_ea(&self) -> bool {
        self.attr_type == AttributeType::Ea.code()
    }

    pub fn is_logged_utility_stream(&self) -> bool {
        self.attr_type == AttributeType::LoggedUtilityStream.code()
    }

    pub fn is_end(&self) -> bool {
        self.attr_type == AttributeType::End.code()
    }
}

/// Resident Attribute Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct ResidentAttributeHeader {
    pub value_length: u32,
    pub value_offset: u16,
    pub indexed: u8,
    pub padding: u8,
}

/// Non-Resident Attribute Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct NonResidentAttributeHeader {
    pub lowest_vcn: u64,
    pub highest_vcn: u64,
    pub data_runs_offset: u16,
    pub compression_unit: u16,
    pub padding: u32,
    pub allocated_size: u64,
    pub data_size: u64,
    pub initialized_size: u64,
}

/// Full Header for Resident Attribute (convenience for reading)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FullResidentAttributeHeader {
    pub common: AttributeHeader,
    pub resident: ResidentAttributeHeader,
}

/// Full Header for Non-Resident Attribute (convenience for reading)
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FullNonResidentAttributeHeader {
    pub common: AttributeHeader,
    pub non_resident: NonResidentAttributeHeader,
}

/// Standard Information Attribute (0x10)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct StandardInformation {
    pub creation_time: u64,
    pub modification_time: u64,
    pub mft_modification_time: u64,
    pub access_time: u64,
    pub file_attributes: u32,
    pub maximum_versions: u32,
    pub version_number: u32,
    pub class_id: u32,
    pub owner_id: u32, // NTFS 3.x
    pub security_id: u32,
    pub quota_charged: u64,
    pub usn: u64,
}

/// File Name Attribute (0x30)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct FileNameAttribute {
    pub parent_directory: u64,
    pub creation_time: u64,
    pub modification_time: u64,
    pub mft_modification_time: u64,
    pub access_time: u64,
    pub allocated_size: u64,
    pub data_size: u64,
    pub file_attributes: u32,
    pub packed_ea_size: u16, // Used for reparse points too
    pub reserved: u16,
    pub filename_length: u8,
    pub namespace: u8,
}

impl FileNameAttribute {
    pub fn new(
        parent_directory: u64,
        data_size: u64,
        file_attributes: NtfsFileAttributes,
        filename_length: u8,
        namespace: u8,
    ) -> Self {
        Self {
            parent_directory,
            creation_time: 0,
            modification_time: 0,
            mft_modification_time: 0,
            access_time: 0,
            allocated_size: data_size,
            data_size,
            file_attributes: file_attributes.bits(),
            packed_ea_size: 0,
            reserved: 0,
            filename_length,
            namespace,
        }
    }
}

/// Index Root Attribute (0x90) Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct IndexRootHeader {
    pub indexed_attr_type: u32, // Usually 0x30 (FILE_NAME)
    pub collation_rule: u32,    // 1 = CollationFileName
    pub index_alloc_entry_size: u32,
    pub clusters_per_index_record: i8,
    pub padding: [u8; 3],
}

/// Index Node Header (used in $INDEX_ROOT and INDX records)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct IndexNodeHeader {
    pub entries_offset: u32,
    pub index_length: u32,
    pub allocated_size: u32,
    pub flags: u8, // 1 = Has Subnodes
    pub padding: [u8; 3],
}
/// $VOLUME_INFORMATION attribute content
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct VolumeInformation {
    /// Reserved (0 for NTFS)
    pub reserved: u64,
    /// Major version
    pub major_version: u8,
    /// Minor version
    pub minor_version: u8,
    /// Volume flags (e.g., Dirty=0x0001)
    pub flags: u16,
}

/// Index entry header (for directory indexes)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct IndexEntryHeader {
    /// MFT reference of the file
    pub mft_reference: u64,
    /// Length of this entry
    pub entry_length: u16,
    /// Length of content (filename attribute)
    pub content_length: u16,
    /// Flags (has sub-node, last entry)
    pub flags: u8,
    /// Padding
    pub padding: [u8; 3],
}

impl IndexEntryHeader {
    pub fn new(mft_ref: u64, content_size: u16, is_last: bool) -> Self {
        Self {
            mft_reference: mft_ref,
            entry_length: 16 + content_size.div_ceil(8) * 8, // Header is 16 bytes, aligned to 8
            content_length: content_size,
            flags: if is_last {
                IndexEntryFlags::LAST_ENTRY.bits()
            } else {
                0
            },
            padding: [0; 3],
        }
    }
}

/// Update Sequence Array element
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct UpdateSequenceArray {
    /// Check value (written to end of each sector)
    pub check: u16,
}

/// Index Record (standard 4KB block in Index Allocation)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct IndexRecordHeader {
    /// Signature: "INDX"
    pub signature: [u8; 4],
    /// Offset to the update sequence
    pub usa_offset: u16,
    /// Size of update sequence in words
    pub usa_count: u16,
    /// LogFile sequence number
    pub lsn: u64,
    /// VCN (Virtual Cluster Number) of this record in the index allocation
    pub index_block_vcn: u64,
}

impl IndexRecordHeader {
    pub fn new(vcn: u64, usa_offset: u16, usa_count: u16) -> Self {
        Self {
            signature: *b"INDX",
            usa_offset,
            usa_count,
            lsn: 0,
            index_block_vcn: vcn,
        }
    }
}

/// NTFS Boot Sector (VBR)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct NtfsBootSector {
    /// Jump instruction (3 bytes)
    pub jump: [u8; 3],
    /// OEM ID: "NTFS    "
    pub oem_id: [u8; 8],

    // BIOS Parameter Block (BPB)
    /// Bytes per sector (typically 512)
    pub bytes_per_sector: u16,
    /// Sectors per cluster (power of 2)
    pub sectors_per_cluster: u8,
    /// Reserved sectors (unused, must be 0)
    pub reserved_sectors: u16,
    /// Always 0 for NTFS
    pub always_zero1: [u8; 3],
    /// Not used (0x0000)
    pub not_used1: u16,
    /// Media descriptor (0xF8 for hard disk)
    pub media_descriptor: u8,
    /// Always 0 for NTFS
    pub always_zero2: u16,
    /// Sectors per track (for CHS, legacy)
    pub sectors_per_track: u16,
    /// Number of heads (for CHS, legacy)
    pub number_of_heads: u16,
    /// Hidden sectors (sectors before partition start)
    pub hidden_sectors: u32,
    /// Not used (0x00000000)
    pub not_used2: u32,

    // Extended BPB
    /// Not used (0x80008000)
    pub not_used3: u32,
    /// Total sectors in volume
    pub total_sectors: u64,
    /// LCN (Logical Cluster Number) of $MFT
    pub mft_lcn: u64,
    /// LCN of $MFTMirr
    pub mft_mirr_lcn: u64,
    /// Clusters per MFT record (can be negative for bytes)
    pub clusters_per_mft_record: i8,
    /// Unused
    pub unused1: [u8; 3],
    /// Clusters per index record (can be negative for bytes)
    pub clusters_per_index_record: i8,
    /// Unused
    pub unused2: [u8; 3],
    /// Volume serial number
    pub volume_serial: u64,
    /// Checksum (unused)
    pub checksum: u32,

    /// Boot code
    pub boot_code: [u8; 426],
    /// End of sector marker (0x55AA)
    pub end_marker: u16,
}

impl NtfsBootSector {
    /// Create a default boot sector from metadata
    pub fn new_from_meta(meta: &crate::meta::NtfsMeta) -> Self {
        let clusters_per_mft_record = if meta.mft_record_size >= meta.bytes_per_cluster {
            (meta.mft_record_size / meta.bytes_per_cluster) as i8
        } else {
            // Negative value represents log2 of size in bytes
            -(meta.mft_record_size.trailing_zeros() as i8)
        };

        let clusters_per_index_record = if meta.index_record_size >= meta.bytes_per_cluster {
            (meta.index_record_size / meta.bytes_per_cluster) as i8
        } else {
            -(meta.index_record_size.trailing_zeros() as i8)
        };

        Self {
            jump: [0xEB, 0x52, 0x90],
            oem_id: *b"NTFS    ",
            bytes_per_sector: meta.bytes_per_sector,
            sectors_per_cluster: meta.sectors_per_cluster,
            reserved_sectors: 0,
            always_zero1: [0; 3],
            not_used1: 0,
            media_descriptor: 0xF8,
            always_zero2: 0,
            sectors_per_track: 63,
            number_of_heads: 255,
            hidden_sectors: meta.hidden_sectors,
            not_used2: 0,
            not_used3: 0x00800080,
            total_sectors: meta.total_sectors,
            mft_lcn: meta.mft_lcn,
            mft_mirr_lcn: meta.mft_mirr_lcn,
            clusters_per_mft_record,
            unused1: [0; 3],
            clusters_per_index_record,
            unused2: [0; 3],
            volume_serial: meta.volume_serial,
            checksum: 0,
            boot_code: [0; 426],
            end_marker: 0xAA55,
        }
    }

    /// Calculate bytes per cluster
    pub fn bytes_per_cluster(&self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }

    /// Calculate MFT record size in bytes
    ///
    /// If clusters_per_mft_record is positive, it's the number of clusters.
    /// If negative, the absolute value is log2 of the size in bytes.
    pub fn mft_record_size(&self) -> u32 {
        if self.clusters_per_mft_record > 0 {
            self.clusters_per_mft_record as u32 * self.bytes_per_cluster()
        } else {
            1u32 << (-self.clusters_per_mft_record as u32)
        }
    }

    /// Calculate index record size in bytes
    pub fn index_record_size(&self) -> u32 {
        if self.clusters_per_index_record > 0 {
            self.clusters_per_index_record as u32 * self.bytes_per_cluster()
        } else {
            1u32 << (-self.clusters_per_index_record as u32)
        }
    }
}
