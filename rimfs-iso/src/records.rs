// SPDX-License-Identifier: MIT
//! ISO fixed records and both-endian integer encodings.
use rimio::{RimIOError, RimIOResult};
use zerocopy::byteorder::{big_endian as be, little_endian as le};
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, Unaligned};

#[derive(
    FromBytes,
    IntoBytes,
    KnownLayout,
    Immutable,
    Unaligned,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[repr(C)]
pub struct BothE16 {
    pub little: le::U16,
    pub big: be::U16,
}
impl From<u16> for BothE16 {
    fn from(value: u16) -> Self {
        Self {
            little: value.into(),
            big: value.into(),
        }
    }
}
impl BothE16 {
    pub fn get(&self) -> RimIOResult<u16> {
        let value = self.little.get();
        if value != self.big.get() {
            return Err(RimIOError::Invalid("ISO both-endian value mismatch"));
        }
        Ok(value)
    }
}

#[derive(
    FromBytes,
    IntoBytes,
    KnownLayout,
    Immutable,
    Unaligned,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
)]
#[repr(C)]
pub struct BothE32 {
    pub little: le::U32,
    pub big: be::U32,
}
impl From<u32> for BothE32 {
    fn from(value: u32) -> Self {
        Self {
            little: value.into(),
            big: value.into(),
        }
    }
}
impl BothE32 {
    pub fn get(&self) -> RimIOResult<u32> {
        let value = self.little.get();
        if value != self.big.get() {
            return Err(RimIOError::Invalid("ISO both-endian value mismatch"));
        }
        Ok(value)
    }
}

#[derive(
    FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(C)]
pub struct IsoVolumeDescriptor {
    pub kind: u8,
    pub standard_id: [u8; 5],
    pub version: u8,
    pub flags: u8,
    pub system_id: [u8; 32],
    pub volume_id: [u8; 32],
    pub unused1: [u8; 8],
    pub volume_space_size: BothE32,
    pub escape_sequences: [u8; 32],
    pub volume_set_size: BothE16,
    pub volume_sequence: BothE16,
    pub logical_block_size: BothE16,
    pub path_table_size: BothE32,
    pub path_table_l: le::U32,
    pub optional_path_table_l: le::U32,
    pub path_table_m: be::U32,
    pub optional_path_table_m: be::U32,
    pub root: IsoRootDirectoryRecord,
    pub volume_set_id: [u8; 128],
    pub publisher_id: [u8; 128],
    pub preparer_id: [u8; 128],
    pub application_id: [u8; 128],
    pub copyright_file: [u8; 37],
    pub abstract_file: [u8; 37],
    pub bibliographic_file: [u8; 37],
    pub created: [u8; 17],
    pub modified: [u8; 17],
    pub expires: [u8; 17],
    pub effective: [u8; 17],
    pub file_structure_version: u8,
    pub reserved1: u8,
    pub application_use: [u8; 512],
    pub reserved2: [u8; 653],
}
impl Default for IsoVolumeDescriptor {
    fn default() -> Self {
        Self::new_zeroed()
    }
}
const _: () = {
    assert!(core::mem::size_of::<IsoVolumeDescriptor>() == 2048);
    assert!(core::mem::align_of::<IsoVolumeDescriptor>() == 1);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, kind) == 0);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, standard_id) == 1);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, version) == 6);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, flags) == 7);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, system_id) == 8);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, volume_id) == 40);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, unused1) == 72);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, volume_space_size) == 80);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, escape_sequences) == 88);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, volume_set_size) == 120);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, volume_sequence) == 124);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, logical_block_size) == 128);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, path_table_size) == 132);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, path_table_l) == 140);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, optional_path_table_l) == 144);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, path_table_m) == 148);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, optional_path_table_m) == 152);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, root) == 156);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, volume_set_id) == 190);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, publisher_id) == 318);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, preparer_id) == 446);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, application_id) == 574);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, copyright_file) == 702);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, abstract_file) == 739);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, bibliographic_file) == 776);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, created) == 813);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, modified) == 830);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, expires) == 847);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, effective) == 864);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, file_structure_version) == 881);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, reserved1) == 882);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, application_use) == 883);
    assert!(core::mem::offset_of!(IsoVolumeDescriptor, reserved2) == 1395);
};

#[derive(
    FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(C)]
pub struct IsoDirectoryHeader {
    pub record_len: u8,
    pub extended_attr_len: u8,
    pub extent_lba: BothE32,
    pub data_length: BothE32,
    pub recorded: [u8; 7],
    pub flags: u8,
    pub file_unit_size: u8,
    pub interleave_gap: u8,
    pub volume_sequence: BothE16,
    pub name_len: u8,
}
impl Default for IsoDirectoryHeader {
    fn default() -> Self {
        Self::new_zeroed()
    }
}
const _: () = {
    assert!(core::mem::size_of::<IsoDirectoryHeader>() == 33);
    assert!(core::mem::align_of::<IsoDirectoryHeader>() == 1);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, record_len) == 0);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, extended_attr_len) == 1);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, extent_lba) == 2);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, data_length) == 10);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, recorded) == 18);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, flags) == 25);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, file_unit_size) == 26);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, interleave_gap) == 27);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, volume_sequence) == 28);
    assert!(core::mem::offset_of!(IsoDirectoryHeader, name_len) == 32);
};

#[derive(
    FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(C)]
pub struct IsoRootDirectoryRecord {
    pub header: IsoDirectoryHeader,
    pub identifier: u8,
}
impl Default for IsoRootDirectoryRecord {
    fn default() -> Self {
        Self::new_zeroed()
    }
}
const _: () = {
    assert!(core::mem::size_of::<IsoRootDirectoryRecord>() == 34);
    assert!(core::mem::align_of::<IsoRootDirectoryRecord>() == 1);
    assert!(core::mem::offset_of!(IsoRootDirectoryRecord, header) == 0);
    assert!(core::mem::offset_of!(IsoRootDirectoryRecord, identifier) == 33);
};

#[derive(
    FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(C)]
pub struct IsoBootDescriptor {
    pub kind: u8,
    pub standard_id: [u8; 5],
    pub version: u8,
    pub system_id: [u8; 32],
    pub boot_id: [u8; 32],
    pub catalog_lba: le::U32,
    pub reserved: [u8; 1973],
}
impl Default for IsoBootDescriptor {
    fn default() -> Self {
        Self::new_zeroed()
    }
}
const _: () = {
    assert!(core::mem::size_of::<IsoBootDescriptor>() == 2048);
    assert!(core::mem::align_of::<IsoBootDescriptor>() == 1);
    assert!(core::mem::offset_of!(IsoBootDescriptor, kind) == 0);
    assert!(core::mem::offset_of!(IsoBootDescriptor, standard_id) == 1);
    assert!(core::mem::offset_of!(IsoBootDescriptor, version) == 6);
    assert!(core::mem::offset_of!(IsoBootDescriptor, system_id) == 7);
    assert!(core::mem::offset_of!(IsoBootDescriptor, boot_id) == 39);
    assert!(core::mem::offset_of!(IsoBootDescriptor, catalog_lba) == 71);
    assert!(core::mem::offset_of!(IsoBootDescriptor, reserved) == 75);
};

#[derive(
    FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, PartialEq, Eq,
)]
#[repr(C)]
pub struct IsoTerminator {
    pub kind: u8,
    pub standard_id: [u8; 5],
    pub version: u8,
    pub reserved: [u8; 2041],
}
impl Default for IsoTerminator {
    fn default() -> Self {
        Self::new_zeroed()
    }
}
const _: () = {
    assert!(core::mem::size_of::<IsoTerminator>() == 2048);
    assert!(core::mem::align_of::<IsoTerminator>() == 1);
    assert!(core::mem::offset_of!(IsoTerminator, kind) == 0);
    assert!(core::mem::offset_of!(IsoTerminator, standard_id) == 1);
    assert!(core::mem::offset_of!(IsoTerminator, version) == 6);
    assert!(core::mem::offset_of!(IsoTerminator, reserved) == 7);
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endian_redundancy_and_record_bounds() {
        let number = BothE32::from(0x12345678);
        assert_eq!(
            number.as_bytes(),
            &[0x78, 0x56, 0x34, 0x12, 0x12, 0x34, 0x56, 0x78]
        );
        assert_eq!(number.get().unwrap(), 0x12345678);
        let bad = BothE16 {
            little: 1.into(),
            big: 2.into(),
        };
        assert!(bad.get().is_err());
        let mut bytes = [0u8; 2048];
        bytes[0] = 1;
        bytes[1..6].copy_from_slice(b"CD001");
        let view = IsoVolumeDescriptor::ref_from_bytes(&bytes).unwrap();
        assert_eq!(view.as_bytes(), bytes);
        assert!(IsoVolumeDescriptor::ref_from_bytes(&bytes[..2047]).is_err());
        let header = IsoDirectoryHeader {
            record_len: 34,
            extent_lba: 20.into(),
            name_len: 1,
            ..Default::default()
        };
        assert_eq!(
            IsoDirectoryHeader::ref_from_bytes(header.as_bytes()).unwrap(),
            &header
        );
        assert!(IsoDirectoryHeader::ref_from_prefix(&[0; 32]).is_err());
    }
}

/// El Torito catalog validation record.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct IsoBootValidationEntry {
    pub header_id: u8,
    pub platform_id: u8,
    pub reserved: [u8; 2],
    pub identifier: [u8; 24],
    pub checksum: le::U16,
    pub key: [u8; 2],
}
impl IsoBootValidationEntry {
    pub fn checksum_sum(&self) -> u16 {
        self.as_bytes().chunks_exact(2).fold(0u16, |sum, word| {
            sum.wrapping_add(u16::from_le_bytes([word[0], word[1]]))
        })
    }
}
/// Initial/default or section boot entry; selection criteria stay uninterpreted.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct IsoCatalogBootEntry {
    pub boot_indicator: u8,
    pub media_type: u8,
    pub load_segment: le::U16,
    pub system_type: u8,
    pub reserved: u8,
    pub sector_count: le::U16,
    pub image_lba: le::U32,
    pub selection_criteria: [u8; 20],
}
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct IsoBootSectionHeader {
    pub indicator: u8,
    pub platform_id: u8,
    pub entry_count: le::U16,
    pub identifier: [u8; 28],
}
const _: () = {
    assert!(core::mem::size_of::<IsoBootValidationEntry>() == 32);
    assert!(core::mem::offset_of!(IsoBootValidationEntry, checksum) == 28);
    assert!(core::mem::offset_of!(IsoBootValidationEntry, key) == 30);
    assert!(core::mem::size_of::<IsoCatalogBootEntry>() == 32);
    assert!(core::mem::offset_of!(IsoCatalogBootEntry, load_segment) == 2);
    assert!(core::mem::offset_of!(IsoCatalogBootEntry, sector_count) == 6);
    assert!(core::mem::offset_of!(IsoCatalogBootEntry, image_lba) == 8);
    assert!(core::mem::offset_of!(IsoCatalogBootEntry, selection_criteria) == 12);
    assert!(core::mem::size_of::<IsoBootSectionHeader>() == 32);
    assert!(core::mem::offset_of!(IsoBootSectionHeader, entry_count) == 2);
    assert!(core::mem::offset_of!(IsoBootSectionHeader, identifier) == 4);
};

/// Fixed prefix of Type L / Type M path-table records; the identifier follows.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy)]
#[repr(C)]
pub struct IsoPathTableHeader<O: zerocopy::byteorder::ByteOrder> {
    pub identifier_len: u8,
    pub extended_attribute_len: u8,
    pub extent_lba: zerocopy::byteorder::U32<O>,
    pub parent_number: zerocopy::byteorder::U16<O>,
}
pub type IsoPathTableHeaderLe = IsoPathTableHeader<zerocopy::byteorder::LittleEndian>;
pub type IsoPathTableHeaderBe = IsoPathTableHeader<zerocopy::byteorder::BigEndian>;
const _: () = {
    assert!(core::mem::size_of::<IsoPathTableHeaderLe>() == 8);
    assert!(core::mem::size_of::<IsoPathTableHeaderBe>() == 8);
    assert!(core::mem::align_of::<IsoPathTableHeaderLe>() == 1);
    assert!(core::mem::align_of::<IsoPathTableHeaderBe>() == 1);
    assert!(core::mem::offset_of!(IsoPathTableHeaderLe, extent_lba) == 2);
    assert!(core::mem::offset_of!(IsoPathTableHeaderLe, parent_number) == 6);
};

#[cfg(test)]
mod catalog_tests {
    use super::*;

    #[test]
    fn boot_catalog_layout_and_checksum() {
        let mut validation = IsoBootValidationEntry {
            header_id: 1,
            platform_id: 0xef,
            key: [0x55, 0xaa],
            ..Default::default()
        };
        validation.checksum = 0u16.wrapping_sub(validation.checksum_sum()).into();
        assert_eq!(validation.checksum_sum(), 0);
        assert_eq!(
            IsoBootValidationEntry::ref_from_bytes(validation.as_bytes())
                .unwrap()
                .as_bytes(),
            validation.as_bytes()
        );
        assert!(IsoBootValidationEntry::ref_from_bytes(&validation.as_bytes()[..31]).is_err());
        let entry = IsoCatalogBootEntry {
            boot_indicator: 0x88,
            sector_count: 0x1234.into(),
            image_lba: 0x12345678.into(),
            ..Default::default()
        };
        assert_eq!(
            &entry.as_bytes()[6..12],
            &[0x34, 0x12, 0x78, 0x56, 0x34, 0x12]
        );
        assert!(IsoCatalogBootEntry::ref_from_bytes(&entry.as_bytes()[..31]).is_err());
        let section = IsoBootSectionHeader {
            indicator: 0x91,
            platform_id: 0xef,
            entry_count: 1.into(),
            ..Default::default()
        };
        assert_eq!(&section.as_bytes()[..4], &[0x91, 0xef, 1, 0]);
        assert!(IsoBootSectionHeader::ref_from_bytes(&section.as_bytes()[..31]).is_err());
    }

    #[test]
    fn path_table_endian_variants_share_layout() {
        let little = IsoPathTableHeaderLe {
            identifier_len: 3,
            extended_attribute_len: 0,
            extent_lba: 0x12345678.into(),
            parent_number: 0x1234.into(),
        };
        let big = IsoPathTableHeaderBe {
            identifier_len: 3,
            extended_attribute_len: 0,
            extent_lba: 0x12345678.into(),
            parent_number: 0x1234.into(),
        };
        assert_eq!(
            little.as_bytes(),
            &[3, 0, 0x78, 0x56, 0x34, 0x12, 0x34, 0x12]
        );
        assert_eq!(big.as_bytes(), &[3, 0, 0x12, 0x34, 0x56, 0x78, 0x12, 0x34]);
        assert_eq!(
            IsoPathTableHeaderLe::ref_from_bytes(little.as_bytes())
                .unwrap()
                .extent_lba
                .get(),
            big.extent_lba.get()
        );
        assert!(IsoPathTableHeaderBe::ref_from_bytes(&big.as_bytes()[..7]).is_err());
    }
}
