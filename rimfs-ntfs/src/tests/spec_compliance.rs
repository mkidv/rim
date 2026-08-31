#![allow(unused_imports)]

// SPDX-License-Identifier: MIT

use crate::types::*;

#[test]
fn test_mft_record_header_layout() {
    // Spec: "The MFT entry header (FILE_RECORD_SEGMENT_HEADER) is 42 or 48 bytes in size"
    // We target NTFS 3.1 which uses 48 bytes.
    assert_eq!(
        core::mem::size_of::<MftRecordHeader>(),
        48,
        "MftRecordHeader must be 48 bytes"
    );

    // Offsets verification (based on packed struct)
    // signature: 0
    // usa_offset: 4
    // usa_count: 6
    // lsn: 8
    // sequence_number: 16
    // link_count: 18
    // attrs_offset: 20
    // flags: 22
    // bytes_used: 24
    // bytes_allocated: 28
    // base_file_record: 32
    // next_attr_id: 40
    // reserved: 42 (NTFS 3.0/3.1 specific)
    // mft_record_number: 44 (NTFS 3.1 specific)
}

#[test]
fn test_attribute_header_layout() {
    // Spec: "The MFT attribute header (ATTRIBUTE_RECORD_HEADER) is 16 bytes in size"
    assert_eq!(
        core::mem::size_of::<AttributeHeader>(),
        16,
        "AttributeHeader must be 16 bytes"
    );
}

#[test]
fn test_resident_attribute_header_layout() {
    // Spec: "The resident data is 8 bytes in size"
    assert_eq!(
        core::mem::size_of::<ResidentAttributeHeader>(),
        8,
        "ResidentAttributeHeader must be 8 bytes"
    );
}

#[test]
fn test_non_resident_attribute_header_layout() {
    assert_eq!(
        core::mem::size_of::<NonResidentAttributeHeader>(),
        48,
        "NonResidentAttributeHeader must be 48 bytes"
    );
}

#[test]
fn test_index_entry_header_layout() {
    // Spec:
    // mft_ref: 8
    // entry_len: 2
    // content_len: 2
    // flags: 1
    // padding: 3
    // Total: 16 bytes.
    assert_eq!(
        core::mem::size_of::<IndexEntryHeader>(),
        16,
        "IndexEntryHeader must be 16 bytes"
    );
}

#[test]
fn test_index_record_header_layout() {
    // Spec:
    // signature: 4
    // usa_offset: 2
    // usa_count: 2
    // lsn: 8
    // index_block_vcn: 8
    // Total: 24 bytes.
    assert_eq!(
        core::mem::size_of::<IndexRecordHeader>(),
        24,
        "IndexRecordHeader must be 24 bytes"
    );
}

#[test]
fn test_ntfs_boot_sector_layout() {
    // Spec: 512 bytes total, but struct might match exactly fields.
    // BootSector is usually 512 bytes including boot code.
    // Struct NtfsBootSector:
    // jump: 3
    // oem: 8
    // bpb: 25 (packed) = 2+1+2+3+2+1+2+2+2+4+4
    // extended bpb: 48 (packed) = 4+8+8+8+1+3+1+3+8+4
    // boot_code: 426
    // end_marker: 2
    // Total: 3+8+25+48+426+2 = 512 bytes
    assert_eq!(
        core::mem::size_of::<NtfsBootSector>(),
        512,
        "NtfsBootSector must be 512 bytes"
    );
}

#[test]
fn test_file_name_attribute_layout() {
    // Spec:
    // parent_dir: 8
    // creation: 8
    // modification: 8
    // mft_mod: 8
    // access: 8
    // allocated: 8
    // data: 8
    // file_attr: 4
    // packed_ea: 2
    // reserved: 2
    // len: 1
    // namespace: 1
    // Total: 8*7 + 4 + 2 + 2 + 1 + 1 = 56 + 10 = 66 bytes.
    assert_eq!(
        core::mem::size_of::<FileNameAttribute>(),
        66,
        "FileNameAttribute must be 66 bytes"
    );
}

#[test]
fn test_attribute_type_codes() {
    // Spec: Attribute types
    // StandardInformation   = 0x10
    // AttributeList         = 0x20
    // FileName              = 0x30
    // ObjectId              = 0x40
    // SecurityDescriptor    = 0x50
    // VolumeName            = 0x60
    // VolumeInformation     = 0x70
    // Data                  = 0x80
    // IndexRoot             = 0x90
    // IndexAllocation       = 0xA0
    // Bitmap                = 0xB0
    // ReparsePoint          = 0xC0
    // EaInformation         = 0xD0
    // Ea                    = 0xE0
    // LoggedUtilityStream   = 0x100
    // End                   = 0xFFFFFFFF

    assert_eq!(AttributeType::StandardInformation.code(), 0x10);
    assert_eq!(AttributeType::AttributeList.code(), 0x20);
    assert_eq!(AttributeType::FileName.code(), 0x30);
    assert_eq!(AttributeType::ObjectId.code(), 0x40);
    assert_eq!(AttributeType::SecurityDescriptor.code(), 0x50);
    assert_eq!(AttributeType::VolumeName.code(), 0x60);
    assert_eq!(AttributeType::VolumeInformation.code(), 0x70);
    assert_eq!(AttributeType::Data.code(), 0x80);
    assert_eq!(AttributeType::IndexRoot.code(), 0x90);
    assert_eq!(AttributeType::IndexAllocation.code(), 0xA0);
    assert_eq!(AttributeType::Bitmap.code(), 0xB0);
    assert_eq!(AttributeType::ReparsePoint.code(), 0xC0);
    assert_eq!(AttributeType::EaInformation.code(), 0xD0);
    assert_eq!(AttributeType::Ea.code(), 0xE0);
    assert_eq!(AttributeType::LoggedUtilityStream.code(), 0x100);
    assert_eq!(AttributeType::End.code(), 0xFFFFFFFF);
}

#[test]
fn test_mft_entry_flags() {
    // Spec: MFT entry flags
    // IN_USE = 0x0001
    // DIRECTORY = 0x0002
    assert_eq!(MftRecordFlags::IN_USE.bits(), 0x0001);
    assert_eq!(MftRecordFlags::IS_DIRECTORY.bits(), 0x0002);
}

#[test]
fn test_attribute_header_flags() {
    // Spec: MFT attribute data flags
    // COMPRESSED = 0x0001
    // ENCRYPTED = 0x4000
    // SPARSE = 0x8000
    assert_eq!(AttributeFlags::COMPRESSED.bits(), 0x0001);
    assert_eq!(AttributeFlags::ENCRYPTED.bits(), 0x4000);
    assert_eq!(AttributeFlags::SPARSE.bits(), 0x8000);
}

#[test]
fn test_index_entry_flags() {
    // Spec: Index entry flags
    // HAS_SUBNODES = 0x01
    // LAST_ENTRY = 0x02
    assert_eq!(IndexEntryFlags::HAS_SUBNODES.bits(), 0x01);
    assert_eq!(IndexEntryFlags::LAST_ENTRY.bits(), 0x02);
}

#[test]
fn test_index_node_flags() {
    // Spec: Index Node Header flags
    // HAS_CHILDREN = 0x01
    assert_eq!(IndexNodeFlags::HAS_CHILDREN.bits(), 0x01);
}

#[test]
fn test_full_resident_attribute_header_layout() {
    // Spec: AttributeHeader (16) + ResidentAttributeHeader (8) = 24 bytes
    assert_eq!(
        core::mem::size_of::<FullResidentAttributeHeader>(),
        24,
        "FullResidentAttributeHeader must be 24 bytes"
    );
}

#[test]
fn test_full_non_resident_attribute_header_layout() {
    // Spec: AttributeHeader (16) + NonResidentAttributeHeader (48) = 64 bytes
    assert_eq!(
        core::mem::size_of::<FullNonResidentAttributeHeader>(),
        64,
        "FullNonResidentAttributeHeader must be 64 bytes"
    );
}

#[test]
fn test_non_resident_header_serialization() {
    use zerocopy::IntoBytes;
    // Create a known non-resident header
    let header = NonResidentAttributeHeader {
        lowest_vcn: 0x1122334455667788,
        highest_vcn: 0x99AABBCCDDEEFF00,
        data_runs_offset: 0x1234,
        compression_unit: 0x5678,
        padding: 0,
        allocated_size: 0xAAAA_BBBB_CCCC_DDDD,
        data_size: 0x1111_2222_3333_4444,
        initialized_size: 0x5555_6666_7777_8888,
    };

    let bytes = header.as_bytes();

    // Check Offsets
    // 0: lowest_vcn (8)
    assert_eq!(&bytes[0..8], &0x1122334455667788u64.to_le_bytes());
    // 8: highest_vcn (8)
    assert_eq!(&bytes[8..16], &0x99AABBCCDDEEFF00u64.to_le_bytes());
    // 16: data_runs_offset (2)
    assert_eq!(&bytes[16..18], &0x1234u16.to_le_bytes());
    // 18: compression_unit (2)
    assert_eq!(&bytes[18..20], &0x5678u16.to_le_bytes());
    // 20: padding (4)
    assert_eq!(&bytes[20..24], &[0, 0, 0, 0]);
    // 24: allocated_size (8)
    assert_eq!(&bytes[24..32], &0xAAAA_BBBB_CCCC_DDDDu64.to_le_bytes());
    // 32: data_size (8)
    assert_eq!(&bytes[32..40], &0x1111_2222_3333_4444u64.to_le_bytes());
    // 40: initialized_size (8)
    assert_eq!(&bytes[40..48], &0x5555_6666_7777_8888u64.to_le_bytes());
}
