// SPDX-License-Identifier: MIT

//! NTFS file record segment ($MFT record) on-disk structures.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, byteorder::little_endian::*};

use crate::attr::NtfsFileAttributes;
use crate::constant::{NTFS_BOOT_SIGNATURE, NTFS_FILE_SIGNATURE, NTFS_INDX_SIGNATURE};
use crate::flags::*;
use crate::types::NtfsAttributeType;

/// MFT Record Header (Multi_Sector_Header)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct MftRecordHeader {
    /// "FILE"
    pub signature: [u8; 4],
    /// Offset to Update Sequence Array
    pub usa_offset: U16,
    /// Size in words of Update Sequence Array
    pub usa_count: U16,
    /// $LogFile Sequence Number
    pub lsn: U64,
    /// Sequence Number (reused)
    pub sequence_number: U16,
    /// Reference Count (Hard links)
    pub link_count: U16,
    /// Offset to first Attribute
    pub attrs_offset: U16,
    /// Flags (IN_USE, DIRECTORY)
    pub flags: U16,
    /// Real size of the FILE record
    pub bytes_used: U32,
    /// Allocated size of the FILE record
    pub bytes_allocated: U32,
    /// File Reference to the Base FILE record
    pub base_file_record: U64,
    /// Next Attribute ID
    pub next_attr_id: U16,
    /// Unused / Padding
    pub reserved: U16,
    /// MFT Record Number (index)
    pub mft_record_number: U32,
}

impl MftRecordHeader {
    pub fn new(record_number: u32, flags: MftRecordFlags, allocated_size: u32) -> Self {
        Self {
            signature: NTFS_FILE_SIGNATURE,
            usa_offset: (48).into(),
            usa_count: (0).into(), // Set by formatter
            lsn: (0).into(),
            sequence_number: (1).into(), // Default, incremented on reuse
            link_count: (1).into(),
            attrs_offset: (0).into(), // Set by formatter
            flags: (flags.bits()).into(),
            bytes_used: (0).into(), // Set by formatter
            bytes_allocated: (allocated_size).into(),
            base_file_record: (0).into(),
            next_attr_id: (0).into(),
            reserved: (0).into(),
            mft_record_number: (record_number).into(),
        }
    }

    pub fn with_sequence_number(mut self, seq: u16) -> Self {
        self.sequence_number = seq.into();
        self
    }

    pub fn is_in_use(&self) -> bool {
        (self.flags.get() & MftRecordFlags::IN_USE.bits()) != 0
    }

    pub fn is_file_record(&self) -> bool {
        self.signature == NTFS_FILE_SIGNATURE
    }

    pub fn is_dir(&self) -> bool {
        (self.flags.get() & MftRecordFlags::IS_DIRECTORY.bits()) != 0
    }

    pub fn is_file(&self) -> bool {
        !self.is_dir()
    }
}

const _: () = {
    assert!(core::mem::align_of::<MftRecordHeader>() == 1);
    assert!(core::mem::offset_of!(MftRecordHeader, attrs_offset) == 20);
    assert!(core::mem::offset_of!(MftRecordHeader, base_file_record) == 32);
    assert!(core::mem::offset_of!(MftRecordHeader, mft_record_number) == 44);
};

/// Generic Attribute Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct AttributeHeader {
    pub attr_type: U32,
    pub length: U32,
    pub non_resident: u8,
    pub name_length: u8,
    pub name_offset: U16,
    pub flags: U16,
    pub attr_id: U16,
}

impl AttributeHeader {
    pub fn is_resident(&self) -> bool {
        self.non_resident == 0
    }

    pub fn is_non_resident(&self) -> bool {
        self.non_resident != 0
    }

    pub fn is_standard_information(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::StandardInformation.code()
    }

    pub fn is_file_name(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::FileName.code()
    }

    pub fn is_data(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::Data.code()
    }

    pub fn is_index_root(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::IndexRoot.code()
    }

    pub fn is_index_allocation(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::IndexAllocation.code()
    }

    pub fn is_bitmap(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::Bitmap.code()
    }

    pub fn is_reparse_point(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::ReparsePoint.code()
    }

    pub fn is_ea_information(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::EaInformation.code()
    }

    pub fn is_ea(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::Ea.code()
    }

    pub fn is_logged_utility_stream(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::LoggedUtilityStream.code()
    }

    pub fn is_end(&self) -> bool {
        self.attr_type.get() == NtfsAttributeType::End.code()
    }
}

const _: () = {
    assert!(core::mem::align_of::<AttributeHeader>() == 1);
    assert!(core::mem::offset_of!(AttributeHeader, name_offset) == 10);
    assert!(core::mem::offset_of!(AttributeHeader, attr_id) == 14);
};

/// Resident Attribute Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct ResidentAttributeHeader {
    pub value_length: U32,
    pub value_offset: U16,
    pub indexed: u8,
    pub padding: u8,
}

/// Non-Resident Attribute Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct NonResidentAttributeHeader {
    pub lowest_vcn: U64,
    pub highest_vcn: U64,
    pub data_runs_offset: U16,
    pub compression_unit: U16,
    pub padding: U32,
    pub allocated_size: U64,
    pub data_size: U64,
    pub initialized_size: U64,
}

const _: () = {
    assert!(core::mem::align_of::<NonResidentAttributeHeader>() == 1);
    assert!(core::mem::offset_of!(NonResidentAttributeHeader, data_runs_offset) == 16);
    assert!(core::mem::offset_of!(NonResidentAttributeHeader, allocated_size) == 24);
};

/// Full Header for Resident Attribute (convenience for reading)
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FullResidentAttributeHeader {
    pub common: AttributeHeader,
    pub resident: ResidentAttributeHeader,
}

/// Full Header for Non-Resident Attribute (convenience for reading)
#[repr(C)]
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct FullNonResidentAttributeHeader {
    pub common: AttributeHeader,
    pub non_resident: NonResidentAttributeHeader,
}
const _: () = {
    assert!(core::mem::align_of::<FullNonResidentAttributeHeader>() == 1);
};

/// Standard Information Attribute (0x10)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct StandardInformationHeader {
    pub creation_time: U64,
    pub modification_time: U64,
    pub mft_modification_time: U64,
    pub access_time: U64,
    pub file_attributes: U32,
    pub maximum_versions: U32,
    pub version_number: U32,
    pub class_id: U32,
}

/// NTFS 3.x standard information: the legacy 48-byte header plus 24 bytes.
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct StandardInformation {
    pub header: StandardInformationHeader,
    pub owner_id: U32,
    pub security_id: U32,
    pub quota_charged: U64,
    pub usn: U64,
}
const _: () = {
    assert!(core::mem::size_of::<StandardInformationHeader>() == 48);
    assert!(core::mem::align_of::<StandardInformationHeader>() == 1);
    assert!(core::mem::offset_of!(StandardInformationHeader, modification_time) == 8);
    assert!(core::mem::offset_of!(StandardInformationHeader, mft_modification_time) == 16);
    assert!(core::mem::offset_of!(StandardInformationHeader, access_time) == 24);
    assert!(core::mem::offset_of!(StandardInformationHeader, file_attributes) == 32);
    assert!(core::mem::offset_of!(StandardInformationHeader, maximum_versions) == 36);
    assert!(core::mem::offset_of!(StandardInformationHeader, version_number) == 40);
    assert!(core::mem::offset_of!(StandardInformationHeader, class_id) == 44);
    assert!(core::mem::size_of::<StandardInformation>() == 72);
    assert!(core::mem::offset_of!(StandardInformation, owner_id) == 48);
    assert!(core::mem::offset_of!(StandardInformation, security_id) == 52);
    assert!(core::mem::offset_of!(StandardInformation, quota_charged) == 56);
    assert!(core::mem::offset_of!(StandardInformation, usn) == 64);
};

/// File Name Attribute (0x30)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct FileNameAttribute {
    pub parent_directory: U64,
    pub creation_time: U64,
    pub modification_time: U64,
    pub mft_modification_time: U64,
    pub access_time: U64,
    pub allocated_size: U64,
    pub data_size: U64,
    pub file_attributes: U32,
    pub packed_ea_size: U16, // Used for reparse points too
    pub reserved: U16,
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
            parent_directory: U64::new(parent_directory),
            creation_time: U64::new(0),
            modification_time: U64::new(0),
            mft_modification_time: U64::new(0),
            access_time: U64::new(0),
            allocated_size: U64::new(data_size),
            data_size: U64::new(data_size),
            file_attributes: U32::new(file_attributes.bits()),
            packed_ea_size: U16::new(0),
            reserved: U16::new(0),
            filename_length,
            namespace,
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<FileNameAttribute>() == 66);
    assert!(core::mem::align_of::<FileNameAttribute>() == 1);
    assert!(core::mem::offset_of!(FileNameAttribute, data_size) == 48);
    assert!(core::mem::offset_of!(FileNameAttribute, filename_length) == 64);
};

/// Index Root Attribute (0x90) Header
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct IndexRootHeader {
    pub indexed_attr_type: U32, // Usually 0x30 (FILE_NAME)
    pub collation_rule: U32,    // 1 = CollationFileName
    pub index_alloc_entry_size: U32,
    pub clusters_per_index_record: i8,
    pub padding: [u8; 3],
}

/// Index Node Header (used in $INDEX_ROOT and INDX records)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct IndexNodeHeader {
    pub entries_offset: U32,
    pub index_length: U32,
    pub allocated_size: U32,
    pub flags: u8, // 1 = Has Subnodes
    pub padding: [u8; 3],
}

/// $VOLUME_INFORMATION attribute content
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct VolumeInformation {
    /// Reserved (0 for NTFS)
    pub reserved: U64,
    /// Major version
    pub major_version: u8,
    /// Minor version
    pub minor_version: u8,
    /// Volume flags (e.g., Dirty=0x0001)
    pub flags: U16,
}

impl VolumeInformation {
    /// Creates a new NTFS 3.1 volume information structure
    pub const fn new_ntfs_3_1(flags: u16) -> Self {
        Self {
            reserved: U64::new(0),
            major_version: 3,
            minor_version: 1,
            flags: U16::new(flags),
        }
    }
}

impl Default for VolumeInformation {
    fn default() -> Self {
        Self::new_ntfs_3_1(crate::flags::NtfsVolumeFlags::empty().bits())
    }
}

const _: () = {
    assert!(core::mem::size_of::<VolumeInformation>() == 12);
};

/// Index entry header (for directory indexes)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct IndexEntryHeader {
    /// MFT reference of the file
    pub mft_reference: U64,
    /// Length of this entry
    pub entry_length: U16,
    /// Length of content (filename attribute)
    pub content_length: U16,
    /// Flags (has sub-node, last entry)
    pub flags: u8,
    /// Padding
    pub padding: [u8; 3],
}

impl IndexEntryHeader {
    pub fn new(mft_ref: u64, content_size: u16, is_last: bool) -> Self {
        Self {
            mft_reference: U64::new(mft_ref),
            entry_length: U16::new(16 + content_size.div_ceil(8) * 8), // Header is 16 bytes, aligned to 8
            content_length: U16::new(content_size),
            flags: if is_last {
                IndexEntryFlags::LAST_ENTRY.bits()
            } else {
                0
            },
            padding: [0; 3],
        }
    }

    /// Creates a standard 16-byte LAST_ENTRY terminator entry with no sub-nodes.
    pub const fn end_marker() -> Self {
        Self {
            mft_reference: U64::new(0),
            entry_length: U16::new(16),
            content_length: U16::new(0),
            flags: IndexEntryFlags::LAST_ENTRY.bits(),
            padding: [0; 3],
        }
    }

    /// Creates a 24-byte LAST_ENTRY terminator entry with a sub-node VCN.
    pub const fn end_marker_with_subnode() -> Self {
        Self {
            mft_reference: U64::new(0),
            entry_length: U16::new(24),
            content_length: U16::new(0),
            flags: IndexEntryFlags::LAST_ENTRY.bits() | IndexEntryFlags::HAS_SUBNODES.bits(),
            padding: [0; 3],
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<IndexEntryHeader>() == 16);
    assert!(core::mem::align_of::<IndexEntryHeader>() == 1);
};

/// Update Sequence Array element
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct UpdateSequenceArray {
    /// Check value (written to end of each sector)
    pub check: U16,
}

/// Index Record (standard 4KB block in Index Allocation)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct IndexRecordHeader {
    /// Signature: "INDX"
    pub signature: [u8; 4],
    /// Offset to the update sequence
    pub usa_offset: U16,
    /// Size of update sequence in words
    pub usa_count: U16,
    /// LogFile sequence number
    pub lsn: U64,
    /// VCN (Virtual Cluster Number) of this record in the index allocation
    pub index_block_vcn: U64,
}

impl IndexRecordHeader {
    pub fn new(vcn: u64, usa_offset: u16, usa_count: u16) -> Self {
        Self {
            signature: NTFS_INDX_SIGNATURE,
            usa_offset: usa_offset.into(),
            usa_count: usa_count.into(),
            lsn: 0.into(),
            index_block_vcn: vcn.into(),
        }
    }
}

const _: () = {
    assert!(core::mem::align_of::<IndexRecordHeader>() == 1);
};

/// NTFS Boot Sector (VBR)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct NtfsBootSector {
    /// Jump instruction (3 bytes)
    pub jump: [u8; 3],
    /// OEM ID: "NTFS    "
    pub oem_id: [u8; 8],

    // BIOS Parameter Block (BPB)
    /// Bytes per sector (typically 512)
    pub bytes_per_sector: U16,
    /// Sectors per cluster (power of 2)
    pub sectors_per_cluster: u8,
    /// Reserved sectors (unused, must be 0)
    pub reserved_sectors: U16,
    /// Always 0 for NTFS
    pub always_zero1: [u8; 3],
    /// Not used (0x0000)
    pub not_used1: U16,
    /// Media descriptor (0xF8 for hard disk)
    pub media_descriptor: u8,
    /// Always 0 for NTFS
    pub always_zero2: U16,
    /// Sectors per track (for CHS, legacy)
    pub sectors_per_track: U16,
    /// Number of heads (for CHS, legacy)
    pub number_of_heads: U16,
    /// Hidden sectors (sectors before partition start)
    pub hidden_sectors: U32,
    /// Not used (0x00000000)
    pub not_used2: U32,

    // Extended BPB
    /// Not used (0x80008000)
    pub not_used3: U32,
    /// Total sectors in volume
    pub total_sectors: U64,
    /// LCN (Logical Cluster Number) of $MFT
    pub mft_lcn: U64,
    /// LCN of $MFTMirr
    pub mft_mirr_lcn: U64,
    /// Clusters per MFT record (can be negative for bytes)
    pub clusters_per_mft_record: i8,
    /// Unused
    pub unused1: [u8; 3],
    /// Clusters per index record (can be negative for bytes)
    pub clusters_per_index_record: i8,
    /// Unused
    pub unused2: [u8; 3],
    /// Volume serial number
    pub volume_serial: U64,
    /// Checksum (unused)
    pub checksum: U32,

    /// Boot code
    pub boot_code: [u8; 426],
    /// End of sector marker (0x55AA)
    pub end_marker: U16,
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
            oem_id: NTFS_BOOT_SIGNATURE,
            bytes_per_sector: (meta.bytes_per_sector).into(),
            sectors_per_cluster: meta.sectors_per_cluster,
            reserved_sectors: (0).into(),
            always_zero1: [0; 3],
            not_used1: (0).into(),
            media_descriptor: 0xF8,
            always_zero2: (0).into(),
            sectors_per_track: (63).into(),
            number_of_heads: (255).into(),
            hidden_sectors: (meta.hidden_sectors).into(),
            not_used2: (0).into(),
            not_used3: (0x00800080).into(),
            total_sectors: (meta.total_sectors).into(),
            mft_lcn: (meta.mft_lcn).into(),
            mft_mirr_lcn: (meta.mft_mirr_lcn).into(),
            clusters_per_mft_record,
            unused1: [0; 3],
            clusters_per_index_record,
            unused2: [0; 3],
            volume_serial: (meta.volume_serial).into(),
            checksum: (0).into(),
            boot_code: [0; 426],
            end_marker: (0xAA55).into(),
        }
    }

    /// Calculate bytes per cluster
    pub fn bytes_per_cluster(&self) -> u32 {
        self.bytes_per_sector.get() as u32 * self.sectors_per_cluster as u32
    }

    /// Calculate MFT record size in bytes; return zero for an invalid encoding.
    ///
    /// If clusters_per_mft_record is positive, it's the number of clusters.
    /// If negative, the absolute value is log2 of the size in bytes.
    pub fn mft_record_size(&self) -> u32 {
        if self.clusters_per_mft_record == 0 {
            0
        } else if self.clusters_per_mft_record > 0 {
            self.clusters_per_mft_record as u32 * self.bytes_per_cluster()
        } else {
            1u32.checked_shl(self.clusters_per_mft_record.unsigned_abs() as u32)
                .unwrap_or(0)
        }
    }

    /// Calculate index record size in bytes; return zero for an invalid encoding.
    pub fn index_record_size(&self) -> u32 {
        if self.clusters_per_index_record == 0 {
            0
        } else if self.clusters_per_index_record > 0 {
            self.clusters_per_index_record as u32 * self.bytes_per_cluster()
        } else {
            1u32.checked_shl(self.clusters_per_index_record.unsigned_abs() as u32)
                .unwrap_or(0)
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<NtfsBootSector>() == 512);
    assert!(core::mem::offset_of!(NtfsBootSector, oem_id) == 3);
    assert!(core::mem::offset_of!(NtfsBootSector, end_marker) == 510);
    assert!(core::mem::size_of::<MftRecordHeader>() == 48);
    assert!(core::mem::size_of::<AttributeHeader>() == 16);
    assert!(core::mem::size_of::<ResidentAttributeHeader>() == 8);
    assert!(core::mem::align_of::<ResidentAttributeHeader>() == 1);
    assert!(core::mem::offset_of!(ResidentAttributeHeader, value_offset) == 4);
    assert!(core::mem::size_of::<NonResidentAttributeHeader>() == 48);
};

/// Fixed header for non-file (view) index entries, whose first eight bytes
/// locate the data payload instead of containing an MFT reference.
#[derive(Debug, Clone, Copy, Default, FromBytes, IntoBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct IndexDataEntryHeader {
    pub data_offset: U16,
    pub data_length: U16,
    pub reserved: [u8; 4],
    pub entry_length: U16,
    pub key_length: U16,
    pub flags: U16,
    pub padding: [u8; 2],
}
const _: () = {
    assert!(core::mem::size_of::<IndexDataEntryHeader>() == 16);
    assert!(core::mem::align_of::<IndexDataEntryHeader>() == 1);
    assert!(core::mem::offset_of!(IndexDataEntryHeader, data_length) == 2);
    assert!(core::mem::offset_of!(IndexDataEntryHeader, reserved) == 4);
    assert!(core::mem::offset_of!(IndexDataEntryHeader, entry_length) == 8);
    assert!(core::mem::offset_of!(IndexDataEntryHeader, key_length) == 10);
    assert!(core::mem::offset_of!(IndexDataEntryHeader, flags) == 12);
    assert!(core::mem::offset_of!(IndexDataEntryHeader, padding) == 14);
};

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::FromZeros;

    #[test]
    fn standard_information_versions_share_little_endian_header() {
        let mut info = StandardInformation::new_zeroed();
        info.header.creation_time = 0x0102030405060708u64.into();
        info.header.file_attributes = 0x12345678u32.into();
        info.security_id = 256u32.into();
        let bytes = info.as_bytes();
        assert_eq!(&bytes[..8], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(&bytes[32..36], &[0x78, 0x56, 0x34, 0x12]);
        let legacy = StandardInformationHeader::ref_from_bytes(&bytes[..48]).unwrap();
        assert_eq!(legacy.as_bytes(), info.header.as_bytes());
        assert!(StandardInformationHeader::ref_from_bytes(&bytes[..47]).is_err());
        assert!(StandardInformation::ref_from_bytes(&bytes[..71]).is_err());
        assert_eq!(
            StandardInformation::ref_from_bytes(bytes)
                .unwrap()
                .as_bytes(),
            bytes
        );
    }

    #[test]
    fn view_index_header_golden_bytes_and_truncation() {
        let bytes = [20, 0, 20, 0, 0, 0, 0, 0, 40, 0, 4, 0, 0, 0, 0, 0];
        let header = IndexDataEntryHeader::ref_from_bytes(&bytes).unwrap();
        assert_eq!(header.data_offset.get(), 20);
        assert_eq!(header.entry_length.get(), 40);
        assert_eq!(header.as_bytes(), bytes);
        for len in 0..16 {
            assert!(IndexDataEntryHeader::ref_from_bytes(&bytes[..len]).is_err());
        }
    }

    #[test]
    fn boot_bytes_and_record_size_encodings() {
        let meta = crate::meta::NtfsMeta::new(100 * 1024 * 1024, None).unwrap();
        let mut boot = NtfsBootSector::new_from_meta(&meta);
        boot.mft_lcn = 0x1122334455667788.into();
        assert_eq!(&boot.as_bytes()[11..13], &[0, 2]);
        assert_eq!(
            &boot.as_bytes()[48..56],
            &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
        assert_eq!(&boot.as_bytes()[510..], &[0x55, 0xaa]);
        let mut bytes = [0; 513];
        bytes[1..].copy_from_slice(boot.as_bytes());
        assert_eq!(
            NtfsBootSector::ref_from_bytes(&bytes[1..])
                .unwrap()
                .mft_lcn
                .get(),
            0x1122334455667788
        );
        for encoding in [0, -32, -128] {
            boot.clusters_per_mft_record = encoding;
            boot.clusters_per_index_record = encoding;
            assert_eq!(boot.mft_record_size(), 0);
            assert_eq!(boot.index_record_size(), 0);
        }
        boot.clusters_per_mft_record = -10;
        boot.clusters_per_index_record = 1;
        assert_eq!(boot.mft_record_size(), 1024);
        assert_eq!(boot.index_record_size(), boot.bytes_per_cluster());
    }

    #[test]
    fn common_attribute_and_index_record_golden_bytes() {
        let attr = AttributeHeader {
            attr_type: 0x80.into(),
            length: 0x12345678.into(),
            non_resident: 1,
            name_length: 2,
            name_offset: 0x3456.into(),
            flags: 0x8000.into(),
            attr_id: 0x1234.into(),
        };
        assert_eq!(
            attr.as_bytes(),
            &[
                0x80, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 1, 2, 0x56, 0x34, 0, 0x80, 0x34, 0x12
            ]
        );
        assert!(attr.is_data());
        let index = IndexRecordHeader::new(0x1122334455667788, 40, 9);
        assert_eq!(
            &index.as_bytes()[..8],
            &[b'I', b'N', b'D', b'X', 40, 0, 9, 0]
        );
        assert_eq!(
            &index.as_bytes()[16..],
            &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
        let mut bytes = [0; 25];
        bytes[1..].copy_from_slice(index.as_bytes());
        assert_eq!(
            IndexRecordHeader::ref_from_bytes(&bytes[1..])
                .unwrap()
                .index_block_vcn
                .get(),
            0x1122334455667788
        );
        assert!(IndexRecordHeader::ref_from_bytes(&bytes[1..24]).is_err());
    }

    #[test]
    fn mft_header_has_explicit_endian_fields_and_unaligned_views() {
        let mut header =
            MftRecordHeader::new(0x12345678, MftRecordFlags::IN_USE, 1024).with_sequence_number(0x1234);
        header.usa_offset = 48.into();
        header.attrs_offset = 56.into();
        header.base_file_record = 0x1122334455667788.into();
        let bytes = header.as_bytes();
        assert_eq!(&bytes[4..6], &[48, 0]);
        assert_eq!(&bytes[16..18], &[0x34, 0x12]);
        assert_eq!(&bytes[20..24], &[56, 0, 1, 0]);
        assert_eq!(&bytes[28..32], &[0, 4, 0, 0]);
        assert_eq!(
            &bytes[32..40],
            &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
        assert_eq!(&bytes[44..48], &[0x78, 0x56, 0x34, 0x12]);
        let mut unaligned = [0; 49];
        unaligned[1..].copy_from_slice(bytes);
        let view = MftRecordHeader::ref_from_bytes(&unaligned[1..]).unwrap();
        assert!(view.is_in_use());
        assert_eq!(view.sequence_number.get(), 0x1234);
        assert!(MftRecordHeader::ref_from_bytes(&unaligned[1..48]).is_err());
    }
}
