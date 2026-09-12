// SPDX-License-Identifier: MIT

//! EXT on-disk specification compliance tests.

use crate::constant::*;
use crate::types::*;
use zerocopy::IntoBytes;

// Compile-time layout guarantees
const _: () = {
    assert!(core::mem::size_of::<ExtSuperblock>() == 1024);
    assert!(core::mem::size_of::<ExtBlockGroupDesc>() == 64);
    assert!(core::mem::size_of::<ExtInode>() == 256);
};

#[test]
fn test_ext_superblock_wire_format_offsets() {
    let meta = crate::meta::ExtMeta::new(32 * 1024 * 1024, Some("EXT4_TEST")).unwrap();
    let mut sb = ExtSuperblock::from_meta(&meta, 0, 0);
    sb.s_magic = EXT_SUPERBLOCK_MAGIC.into();

    let bytes = sb.as_bytes();
    assert_eq!(&bytes[0..4], &(meta.inode_count as u32).to_le_bytes(), "Inodes count at 0x00");
    assert_eq!(&bytes[4..8], &(meta.block_count as u32).to_le_bytes(), "Blocks count at 0x04");
    assert_eq!(&bytes[56..58], &0xEF53u16.to_le_bytes(), "Magic 0xEF53 at 0x38");
    assert_eq!(&bytes[84..88], &11u32.to_le_bytes(), "First inode 11 at 0x54");
    assert_eq!(&bytes[88..90], &256u16.to_le_bytes(), "Inode size 256 at 0x58");
}

#[test]
fn test_ext_canonical_magic_and_constants() {
    assert_eq!(EXT_SUPERBLOCK_MAGIC, 0xEF53);
    assert_eq!(EXT_SUPERBLOCK_OFFSET, 1024);
}
