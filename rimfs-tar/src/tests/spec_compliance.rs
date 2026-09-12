// SPDX-License-Identifier: MIT

//! TAR (USTAR / POSIX.1-1988) on-disk specification compliance tests.

use crate::types::*;
use zerocopy::IntoBytes;

// Compile-time layout guarantees
const _: () = {
    assert!(core::mem::size_of::<UstarHeader>() == 512);
};

#[test]
fn test_tar_ustar_header_wire_format_offsets() {
    let mut header = UstarHeader::default();
    header.name[0..4].copy_from_slice(b"test");
    header.magic = *b"ustar\0";
    header.version = *b"00";
    header.typeflag = b'0';

    let bytes = header.as_bytes();
    assert_eq!(bytes.len(), 512, "TAR block size must be 512 bytes");
    assert_eq!(&bytes[0..4], b"test", "File name starts at offset 0");
    assert_eq!(&bytes[156..157], b"0", "Typeflag at offset 156");
    assert_eq!(&bytes[257..263], b"ustar\0", "USTAR magic at offset 257");
    assert_eq!(&bytes[263..265], b"00", "USTAR version at offset 263");
}

#[test]
fn test_tar_typeflags_canonical_constants() {
    assert_eq!(REGTYPE, b'0');
    assert_eq!(LNKTYPE, b'1');
    assert_eq!(SYMTYPE, b'2');
    assert_eq!(DIRTYPE, b'5');
}
