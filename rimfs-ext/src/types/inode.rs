// SPDX-License-Identifier: MIT
//! EXT Inode structure

use zerocopy::byteorder::little_endian::{U16, U32};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::attr::ExtFileAttributesExt;
use crate::constant::*;
use crate::core::utils::time_utils::TimeConversion;
use crate::types::{
    block_map::BlockMapArray,
    extent::{ExtExtent, ExtExtentHeader},
};

/// Fixed legacy inode header (128 bytes)
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ExtInodeHeader {
    pub i_mode: U16,
    pub i_uid: U16,
    pub i_size_lo: U32,
    pub i_atime: U32,
    pub i_ctime: U32,
    pub i_mtime: U32,
    pub i_dtime: U32,
    pub i_gid: U16,
    pub i_links_count: U16,
    pub i_blocks_lo: U32,
    pub i_flags: U32,
    pub i_osd1: U32,
    pub i_block: [u8; 60],
    pub i_generation: U32,
    pub i_file_acl_lo: U32,
    pub i_size_high: U32,
    pub i_obso_faddr: U32,
    pub i_osd2: [u8; 12],
}

/// Extended inode containing the canonical 128-byte legacy header.
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
pub struct ExtInode {
    pub header: ExtInodeHeader,
    pub i_extra_isize: U16,
    pub i_checksum_hi: U16,
    pub i_ctime_extra: U32,
    pub i_mtime_extra: U32,
    pub i_atime_extra: U32,
    pub i_crtime: U32,
    pub i_crtime_extra: U32,
    pub i_version_hi: U32,
    pub i_projid: U32,
    pub i_reserved: [u8; 96],
}

impl Default for ExtInodeHeader {
    fn default() -> Self {
        Self {
            i_mode: 0.into(),
            i_uid: EXT_DEFAULT_UID.into(),
            i_size_lo: 0.into(),
            i_atime: 0.into(),
            i_ctime: 0.into(),
            i_mtime: 0.into(),
            i_dtime: 0.into(),
            i_gid: EXT_DEFAULT_GID.into(),
            i_links_count: 0.into(),
            i_blocks_lo: 0.into(),
            i_flags: EXT_INODE_FLAG_EXTENTS.into(),
            i_osd1: 0.into(),
            i_block: [0; 60],
            i_generation: 0.into(),
            i_file_acl_lo: 0.into(),
            i_size_high: 0.into(),
            i_obso_faddr: 0.into(),
            i_osd2: [0; 12],
        }
    }
}
impl Default for ExtInode {
    fn default() -> Self {
        Self {
            header: ExtInodeHeader::default(),
            i_extra_isize: 32.into(),
            i_checksum_hi: 0.into(),
            i_ctime_extra: 0.into(),
            i_mtime_extra: 0.into(),
            i_atime_extra: 0.into(),
            i_crtime: 0.into(),
            i_crtime_extra: 0.into(),
            i_version_hi: 0.into(),
            i_projid: 0.into(),
            i_reserved: [0; 96],
        }
    }
}
impl core::ops::Deref for ExtInode {
    type Target = ExtInodeHeader;
    fn deref(&self) -> &Self::Target {
        &self.header
    }
}
impl core::ops::DerefMut for ExtInode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.header
    }
}
const _: () = {
    assert!(core::mem::size_of::<ExtInodeHeader>() == 128);
    assert!(core::mem::align_of::<ExtInodeHeader>() == 1);
    assert!(core::mem::offset_of!(ExtInodeHeader, i_block) == 40);
    assert!(core::mem::offset_of!(ExtInodeHeader, i_size_high) == 108);
    assert!(core::mem::offset_of!(ExtInodeHeader, i_osd2) == 116);
    assert!(core::mem::size_of::<ExtInode>() == 256);
    assert!(core::mem::offset_of!(ExtInode, i_extra_isize) == 128);
};

impl ExtInode {
    /// Create a new inode for a directory
    pub fn new_dir(mode: u16, links: u16, block: u32, extent: ExtExtent) -> Self {
        let mut inode = Self {
            header: ExtInodeHeader {
                i_mode: mode.into(),
                i_links_count: links.into(),
                i_blocks_lo: block.into(),
                i_size_lo: 4096.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        inode.set_extent(extent);
        inode
    }

    /// Create a new inode for a regular file
    pub fn new_file(mode: u16, size: u32, blocks: u32, extent: ExtExtent) -> Self {
        let mut inode = Self {
            header: ExtInodeHeader {
                i_mode: mode.into(),
                i_links_count: 1.into(),
                i_blocks_lo: blocks.into(),
                i_size_lo: size.into(),
                ..Default::default()
            },
            ..Default::default()
        };
        inode.set_extent(extent);
        inode
    }

    /// Set the extent header and first extent in i_block
    pub fn set_extent(&mut self, extent: ExtExtent) {
        let header = ExtExtentHeader {
            eh_entries: 1.into(),
            ..Default::default()
        };

        self.i_block[0..12].copy_from_slice(header.as_bytes());

        self.i_block[12..24].copy_from_slice(extent.as_bytes());
    }

    /// Set multiple extents (up to 4 inline in inode body)
    pub fn set_extents(&mut self, extents: &[ExtExtent]) {
        assert!(
            extents.len() <= 4,
            "ExtInode::set_extents: cannot store more than 4 extents inline in inode body; use extent index tree"
        );
        let count = extents.len() as u16;

        let header = ExtExtentHeader {
            eh_entries: count.into(),
            ..Default::default()
        };

        self.i_block[0..12].copy_from_slice(header.as_bytes());

        for (i, extent) in extents.iter().enumerate() {
            let offset = 12 + i * 12;
            self.i_block[offset..offset + 12].copy_from_slice(extent.as_bytes());
        }
    }

    /// Set block map (Direct/Indirect)
    pub fn set_block_map(&mut self, map: &BlockMapArray) {
        self.i_block[0..60].copy_from_slice(map.as_bytes());
        // BlockMap does not use EXT_INODE_FLAG_EXTENTS, so we must clear it.
        self.i_flags = (self.i_flags.get() & !EXT_INODE_FLAG_EXTENTS).into();
    }

    /// Set timestamps
    pub fn set_timestamps(&mut self, atime: u32, ctime: u32, mtime: u32) {
        self.i_atime = atime.into();
        self.i_ctime = ctime.into();
        self.i_mtime = mtime.into();
    }

    /// Check if this is a directory
    pub fn is_dir(&self) -> bool {
        (self.i_mode.get() & 0xF000) == 0x4000
    }

    /// Check if this is a regular file
    pub fn is_file(&self) -> bool {
        (self.i_mode.get() & 0xF000) == 0x8000
    }

    /// Check if this is a symbolic link
    pub fn is_symlink(&self) -> bool {
        (self.i_mode.get() & 0xF000) == 0xA000
    }

    /// Create an inode from FileAttributes
    /// This is the main constructor for injecting files/directories/slow symlinks
    pub fn from_attr(
        attr: &crate::core::traits::FileAttributes,
        size: u64,
        links: u16,
        blocks: u32,
        extents: &[ExtExtent],
    ) -> Self {
        let i_mode = attr.as_ext4_mode().bits();

        let default_time = time::OffsetDateTime::UNIX_EPOCH;

        let atime = attr.accessed.unwrap_or(default_time).to_unix_u32();
        let ctime = attr.created.unwrap_or(default_time).to_unix_u32();
        let mtime = attr.modified.unwrap_or(default_time).to_unix_u32();

        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        let i_uid = (uid & 0xFFFF) as u16;
        let i_gid = (gid & 0xFFFF) as u16;
        let mut osd2 = [0u8; 12];
        osd2[4..6].copy_from_slice(&((uid >> 16) as u16).to_le_bytes());
        osd2[6..8].copy_from_slice(&((gid >> 16) as u16).to_le_bytes());

        let size_high = if (i_mode & 0xF000) != 0x4000 {
            // Not a directory, handle high 32-bit sizing
            (size >> 32) as u32
        } else {
            0
        };

        let mut inode = Self {
            header: ExtInodeHeader {
                i_mode: i_mode.into(),
                i_uid: i_uid.into(),
                i_gid: i_gid.into(),
                i_size_lo: ((size & 0xFFFF_FFFF) as u32).into(),
                i_size_high: size_high.into(),
                i_links_count: links.into(),
                i_blocks_lo: blocks.into(),
                i_atime: atime.into(),
                i_ctime: ctime.into(),
                i_mtime: mtime.into(),
                i_osd2: osd2,
                ..Default::default()
            },
            ..Default::default()
        };
        inode.set_extents(extents);

        inode
    }

    /// Create a fast symlink inode (target length < 60 bytes)
    pub fn new_fast_symlink(attr: &crate::core::traits::FileAttributes, target: &str) -> Self {
        let mut symlink_attr = attr.clone();
        symlink_attr.kind = crate::core::traits::NodeKind::Symlink;
        let i_mode = symlink_attr.as_ext4_mode().bits();

        let default_time = time::OffsetDateTime::UNIX_EPOCH;

        let atime = symlink_attr.accessed.unwrap_or(default_time).to_unix_u32();
        let ctime = symlink_attr.created.unwrap_or(default_time).to_unix_u32();
        let mtime = symlink_attr.modified.unwrap_or(default_time).to_unix_u32();

        let uid = symlink_attr.uid.unwrap_or(0);
        let gid = symlink_attr.gid.unwrap_or(0);
        let i_uid = (uid & 0xFFFF) as u16;
        let i_gid = (gid & 0xFFFF) as u16;
        let mut osd2 = [0u8; 12];
        osd2[4..6].copy_from_slice(&((uid >> 16) as u16).to_le_bytes());
        osd2[6..8].copy_from_slice(&((gid >> 16) as u16).to_le_bytes());

        let target_bytes = target.as_bytes();
        let len = target_bytes.len();
        let mut i_block = [0u8; 60];
        i_block[..len].copy_from_slice(target_bytes);

        Self {
            header: ExtInodeHeader {
                i_mode: i_mode.into(),
                i_uid: i_uid.into(),
                i_gid: i_gid.into(),
                i_size_lo: (len as u32).into(),
                i_size_high: 0.into(),
                i_links_count: 1.into(),
                i_blocks_lo: 0.into(), // MUST be 0 blocks for fast symlink
                i_flags: 0.into(),     // MUST NOT set EXT_INODE_FLAG_EXTENTS
                i_block,
                i_atime: atime.into(),
                i_ctime: ctime.into(),
                i_mtime: mtime.into(),
                i_osd2: osd2,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create an inode from FileAttributes with BlockMap
    pub fn from_attr_block_map(
        attr: &crate::core::traits::FileAttributes,
        size: u64,
        links: u16,
        blocks: u32,
        map: &BlockMapArray,
    ) -> Self {
        let i_mode = attr.as_ext4_mode().bits();

        #[cfg(feature = "std")]
        let now = time::OffsetDateTime::now_utc();
        #[cfg(not(feature = "std"))]
        let now = time::OffsetDateTime::UNIX_EPOCH;

        let atime = attr.accessed.unwrap_or(now).to_unix_u32();
        let ctime = attr.created.unwrap_or(now).to_unix_u32();
        let mtime = attr.modified.unwrap_or(now).to_unix_u32();

        let uid = attr.uid.unwrap_or(0);
        let gid = attr.gid.unwrap_or(0);
        let i_uid = (uid & 0xFFFF) as u16;
        let i_gid = (gid & 0xFFFF) as u16;
        let mut osd2 = [0u8; 12];
        osd2[4..6].copy_from_slice(&((uid >> 16) as u16).to_le_bytes());
        osd2[6..8].copy_from_slice(&((gid >> 16) as u16).to_le_bytes());

        let size_high = if (i_mode & 0xF000) != 0x4000 {
            (size >> 32) as u32
        } else {
            0
        };

        let mut inode = Self {
            header: ExtInodeHeader {
                i_mode: i_mode.into(),
                i_uid: i_uid.into(),
                i_gid: i_gid.into(),
                i_size_lo: ((size & 0xFFFF_FFFF) as u32).into(),
                i_size_high: size_high.into(),
                i_links_count: links.into(),
                i_blocks_lo: blocks.into(),
                i_atime: atime.into(),
                i_ctime: ctime.into(),
                i_mtime: mtime.into(),
                i_osd2: osd2,
                ..Default::default()
            },
            ..Default::default()
        };
        inode.set_block_map(map);

        inode
    }

    /// Encode to raw bytes (256 bytes)
    pub fn to_bytes(&self) -> [u8; EXT_DEFAULT_INODE_SIZE as usize] {
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("ExtInode size mismatch")
    }
}

const _: () = assert!(core::mem::size_of::<ExtInode>() == EXT_DEFAULT_INODE_SIZE as usize);

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn inode_numeric_fields_have_little_endian_bytes() {
        let mut inode = ExtInode::new_file(0x81a4, 0x12345678, 8, ExtExtent::new(0, 42, 1));
        inode.i_size_high = 0x11223344.into();
        inode.set_timestamps(0x12345678, 1, 2);
        let bytes = inode.as_bytes();
        assert_eq!(&bytes[..2], &[0xa4, 0x81]);
        assert_eq!(
            &bytes[4..12],
            &[0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12]
        );
        assert_eq!(&bytes[108..112], &[0x44, 0x33, 0x22, 0x11]);
        assert_eq!(&bytes[128..130], &[32, 0]);
        let mut map = BlockMapArray::default();
        map.direct[0] = 0x12345678.into();
        inode.set_block_map(&map);
        assert_eq!(&inode.as_bytes()[40..44], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(inode.i_flags.get() & EXT_INODE_FLAG_EXTENTS, 0);
    }

    #[test]
    fn legacy_header_is_shared_with_extended_inode() {
        let inode = ExtInode::new_file(0x81a4, 1234, 8, ExtExtent::new(0, 42, 1));
        let bytes = inode.as_bytes();
        let (header, tail) = ExtInodeHeader::ref_from_prefix(bytes).unwrap();
        assert_eq!(header.as_bytes(), inode.header.as_bytes());
        assert_eq!(tail.len(), 128);
        assert_eq!(
            ExtInodeHeader::ref_from_bytes(&bytes[..128])
                .unwrap()
                .as_bytes(),
            &bytes[..128]
        );
        assert!(ExtInodeHeader::ref_from_bytes(&bytes[..127]).is_err());
        let mut unaligned = [0u8; 129];
        unaligned[1..].copy_from_slice(&bytes[..128]);
        assert!(ExtInodeHeader::ref_from_bytes(&unaligned[1..]).is_ok());
    }
}
