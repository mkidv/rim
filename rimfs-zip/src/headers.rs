// SPDX-License-Identifier: MIT
//! Exact ZIP fixed headers. Variable filenames, extras and comments follow these records.
use zerocopy::byteorder::little_endian::{U16, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, Unaligned};

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
pub struct ZipLocalFileHeader {
    pub signature: U32,
    pub version_needed: U16,
    pub flags: U16,
    pub compression_method: U16,
    pub mtime: U16,
    pub mdate: U16,
    pub crc32: U32,
    pub compressed_size: U32,
    pub uncompressed_size: U32,
    pub name_len: U16,
    pub extra_len: U16,
}
const _: () = {
    assert!(core::mem::size_of::<ZipLocalFileHeader>() == 30);
    assert!(core::mem::align_of::<ZipLocalFileHeader>() == 1);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, signature) == 0);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, version_needed) == 4);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, flags) == 6);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, compression_method) == 8);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, mtime) == 10);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, mdate) == 12);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, crc32) == 14);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, compressed_size) == 18);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, uncompressed_size) == 22);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, name_len) == 26);
    assert!(core::mem::offset_of!(ZipLocalFileHeader, extra_len) == 28);
};

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
pub struct ZipCentralDirectoryHeader {
    pub signature: U32,
    pub version_made_by: U16,
    pub version_needed: U16,
    pub flags: U16,
    pub compression_method: U16,
    pub mtime: U16,
    pub mdate: U16,
    pub crc32: U32,
    pub compressed_size: U32,
    pub uncompressed_size: U32,
    pub name_len: U16,
    pub extra_len: U16,
    pub comment_len: U16,
    pub disk_start: U16,
    pub internal_attributes: U16,
    pub external_attributes: U32,
    pub local_header_offset: U32,
}
const _: () = {
    assert!(core::mem::size_of::<ZipCentralDirectoryHeader>() == 46);
    assert!(core::mem::align_of::<ZipCentralDirectoryHeader>() == 1);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, signature) == 0);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, version_made_by) == 4);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, version_needed) == 6);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, flags) == 8);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, compression_method) == 10);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, mtime) == 12);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, mdate) == 14);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, crc32) == 16);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, compressed_size) == 20);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, uncompressed_size) == 24);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, name_len) == 28);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, extra_len) == 30);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, comment_len) == 32);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, disk_start) == 34);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, internal_attributes) == 36);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, external_attributes) == 38);
    assert!(core::mem::offset_of!(ZipCentralDirectoryHeader, local_header_offset) == 42);
};

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
pub struct ZipEocd {
    pub signature: U32,
    pub disk_number: U16,
    pub directory_disk: U16,
    pub disk_entries: U16,
    pub total_entries: U16,
    pub directory_size: U32,
    pub directory_offset: U32,
    pub comment_len: U16,
}
const _: () = {
    assert!(core::mem::size_of::<ZipEocd>() == 22);
    assert!(core::mem::align_of::<ZipEocd>() == 1);
    assert!(core::mem::offset_of!(ZipEocd, signature) == 0);
    assert!(core::mem::offset_of!(ZipEocd, disk_number) == 4);
    assert!(core::mem::offset_of!(ZipEocd, directory_disk) == 6);
    assert!(core::mem::offset_of!(ZipEocd, disk_entries) == 8);
    assert!(core::mem::offset_of!(ZipEocd, total_entries) == 10);
    assert!(core::mem::offset_of!(ZipEocd, directory_size) == 12);
    assert!(core::mem::offset_of!(ZipEocd, directory_offset) == 16);
    assert!(core::mem::offset_of!(ZipEocd, comment_len) == 20);
};

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
pub struct Zip64Eocd {
    pub signature: U32,
    pub record_size: U64,
    pub version_made_by: U16,
    pub version_needed: U16,
    pub disk_number: U32,
    pub directory_disk: U32,
    pub disk_entries: U64,
    pub total_entries: U64,
    pub directory_size: U64,
    pub directory_offset: U64,
}
const _: () = {
    assert!(core::mem::size_of::<Zip64Eocd>() == 56);
    assert!(core::mem::align_of::<Zip64Eocd>() == 1);
    assert!(core::mem::offset_of!(Zip64Eocd, signature) == 0);
    assert!(core::mem::offset_of!(Zip64Eocd, record_size) == 4);
    assert!(core::mem::offset_of!(Zip64Eocd, version_made_by) == 12);
    assert!(core::mem::offset_of!(Zip64Eocd, version_needed) == 14);
    assert!(core::mem::offset_of!(Zip64Eocd, disk_number) == 16);
    assert!(core::mem::offset_of!(Zip64Eocd, directory_disk) == 20);
    assert!(core::mem::offset_of!(Zip64Eocd, disk_entries) == 24);
    assert!(core::mem::offset_of!(Zip64Eocd, total_entries) == 32);
    assert!(core::mem::offset_of!(Zip64Eocd, directory_size) == 40);
    assert!(core::mem::offset_of!(Zip64Eocd, directory_offset) == 48);
};

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
pub struct Zip64Locator {
    pub signature: U32,
    pub directory_disk: U32,
    pub eocd_offset: U64,
    pub total_disks: U32,
}
const _: () = {
    assert!(core::mem::size_of::<Zip64Locator>() == 20);
    assert!(core::mem::align_of::<Zip64Locator>() == 1);
    assert!(core::mem::offset_of!(Zip64Locator, signature) == 0);
    assert!(core::mem::offset_of!(Zip64Locator, directory_disk) == 4);
    assert!(core::mem::offset_of!(Zip64Locator, eocd_offset) == 8);
    assert!(core::mem::offset_of!(Zip64Locator, total_disks) == 16);
};

#[cfg(test)]
mod tests {
    use super::*;
    fn roundtrip<T: FromBytes + IntoBytes + KnownLayout + Immutable + Unaligned>(len: usize) {
        let bytes: alloc::vec::Vec<u8> = (0..len).map(|n| (n * 17) as u8).collect();
        let view = T::ref_from_bytes(&bytes).unwrap();
        assert_eq!(view.as_bytes(), bytes);
        assert!(T::ref_from_bytes(&bytes[..len - 1]).is_err());
    }
    #[test]
    fn exact_header_roundtrips_and_truncation() {
        roundtrip::<ZipLocalFileHeader>(30);
        roundtrip::<ZipCentralDirectoryHeader>(46);
        roundtrip::<ZipEocd>(22);
        roundtrip::<Zip64Eocd>(56);
        roundtrip::<Zip64Locator>(20);
        let header = ZipLocalFileHeader {
            signature: 0x04034b50.into(),
            compressed_size: 0x12345678.into(),
            ..Default::default()
        };
        assert_eq!(&header.as_bytes()[..4], b"PK\x03\x04");
        assert_eq!(&header.as_bytes()[18..22], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(
            ZipLocalFileHeader::ref_from_bytes(header.as_bytes()).unwrap(),
            &header
        );
    }
}

/// Fixed prefix shared by all variable-length ZIP extra fields.
#[derive(FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned, Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ZipExtraFieldHeader {
    pub id: zerocopy::byteorder::little_endian::U16,
    pub data_len: zerocopy::byteorder::little_endian::U16,
}
const _: () = {
    assert!(core::mem::size_of::<ZipExtraFieldHeader>() == 4);
    assert!(core::mem::align_of::<ZipExtraFieldHeader>() == 1);
    assert!(core::mem::offset_of!(ZipExtraFieldHeader, data_len) == 2);
};

#[cfg(test)]
mod extra_header_tests {
    use super::*;
    #[test]
    fn extra_header_golden_and_truncated() {
        let bytes = [0x55, 0x54, 5, 0];
        let header = ZipExtraFieldHeader::ref_from_bytes(&bytes).unwrap();
        assert_eq!(header.id.get(), 0x5455);
        assert_eq!(header.data_len.get(), 5);
        assert_eq!(header.as_bytes(), bytes);
        for len in 0..4 {
            assert!(ZipExtraFieldHeader::ref_from_bytes(&bytes[..len]).is_err());
        }
    }
}
