// SPDX-License-Identifier: MIT
//! Exact 512-byte POSIX USTAR header, also used by GNU long-name extensions.
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout, Unaligned};
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, KnownLayout, Immutable, Unaligned,
)]
#[repr(C)]
pub struct UstarHeader {
    pub name: [u8; 100],
    pub mode: [u8; 8],
    pub uid: [u8; 8],
    pub gid: [u8; 8],
    pub size: [u8; 12],
    pub mtime: [u8; 12],
    pub checksum: [u8; 8],
    pub typeflag: u8,
    pub link_name: [u8; 100],
    pub magic: [u8; 6],
    pub version: [u8; 2],
    pub uname: [u8; 32],
    pub gname: [u8; 32],
    pub device_major: [u8; 8],
    pub device_minor: [u8; 8],
    pub prefix: [u8; 155],
    pub reserved: [u8; 12],
}
impl Default for UstarHeader {
    fn default() -> Self {
        Self::new_zeroed()
    }
}
impl UstarHeader {
    pub fn calculate_checksum(&self) -> u32 {
        super::types::calculate_checksum(
            self.as_bytes().try_into().expect("exact USTAR header size"),
        )
    }
    pub fn is_zero(&self) -> bool {
        self.as_bytes().iter().all(|&b| b == 0)
    }
}
const _: () = {
    assert!(core::mem::size_of::<UstarHeader>() == 512);
    assert!(core::mem::align_of::<UstarHeader>() == 1);
    assert!(core::mem::offset_of!(UstarHeader, name) == 0);
    assert!(core::mem::offset_of!(UstarHeader, mode) == 100);
    assert!(core::mem::offset_of!(UstarHeader, uid) == 108);
    assert!(core::mem::offset_of!(UstarHeader, gid) == 116);
    assert!(core::mem::offset_of!(UstarHeader, size) == 124);
    assert!(core::mem::offset_of!(UstarHeader, mtime) == 136);
    assert!(core::mem::offset_of!(UstarHeader, checksum) == 148);
    assert!(core::mem::offset_of!(UstarHeader, typeflag) == 156);
    assert!(core::mem::offset_of!(UstarHeader, link_name) == 157);
    assert!(core::mem::offset_of!(UstarHeader, magic) == 257);
    assert!(core::mem::offset_of!(UstarHeader, version) == 263);
    assert!(core::mem::offset_of!(UstarHeader, uname) == 265);
    assert!(core::mem::offset_of!(UstarHeader, gname) == 297);
    assert!(core::mem::offset_of!(UstarHeader, device_major) == 329);
    assert!(core::mem::offset_of!(UstarHeader, device_minor) == 337);
    assert!(core::mem::offset_of!(UstarHeader, prefix) == 345);
    assert!(core::mem::offset_of!(UstarHeader, reserved) == 500);
};
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_roundtrip_and_borrowed_bounds() {
        let bytes = [b'1'; 512];
        let header = UstarHeader::ref_from_bytes(&bytes).unwrap();
        assert_eq!(header.as_bytes(), &bytes);
        assert!(UstarHeader::ref_from_bytes(&bytes[..511]).is_err());
        let mut header = UstarHeader {
            magic: *crate::types::USTAR_MAGIC,
            ..Default::default()
        };
        crate::types::format_octal(&mut header.size, 513);
        assert_eq!(&header.as_bytes()[124..136], b"00000001001\0");
        assert_eq!(&header.as_bytes()[257..263], b"ustar\0");
        assert_eq!(
            UstarHeader::ref_from_bytes(header.as_bytes()).unwrap(),
            &header
        );
    }
}
