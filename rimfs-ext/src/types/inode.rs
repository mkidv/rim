// SPDX-License-Identifier: MIT
//! EXT Inode structure

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::attr::ExtFileAttributesExt;
use crate::constant::*;
use crate::core::utils::time_utils::TimeConversion;
use crate::types::{
    block_map::BlockMapArray,
    extent::{ExtExtent, ExtExtentHeader},
};

/// EXT Inode structure (256 bytes default)
///
/// This represents the on-disk inode format for EXT filesystems.
/// The structure uses extents for block mapping (EXT_INODE_FLAG_EXTENTS).
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C, packed)]
pub struct ExtInode {
    /// File mode (type + permissions)
    pub i_mode: u16,
    /// Owner UID (lower 16 bits)
    pub i_uid: u16,
    /// File size in bytes (lower 32 bits)
    pub i_size_lo: u32,
    /// Last access time (seconds since epoch)
    pub i_atime: u32,
    /// Inode change time (seconds since epoch)
    pub i_ctime: u32,
    /// Last modification time (seconds since epoch)
    pub i_mtime: u32,
    /// Deletion time (0 if not deleted)
    pub i_dtime: u32,
    /// Group GID (lower 16 bits)
    pub i_gid: u16,
    /// Hard links count
    pub i_links_count: u16,
    /// Block count (lower 32 bits, in 512-byte units)
    pub i_blocks_lo: u32,
    /// Inode flags (e.g., EXT_INODE_FLAG_EXTENTS)
    pub i_flags: u32,
    /// OS-specific value 1
    pub i_osd1: u32,
    /// Block mapping / extent tree (60 bytes)
    /// Contains extent header (12 bytes) + up to 4 extents (12 bytes each)
    pub i_block: [u8; 60],
    /// File version (for NFS)
    pub i_generation: u32,
    /// File ACL (lower 32 bits)
    pub i_file_acl_lo: u32,
    /// File size in bytes (upper 32 bits) / Directory ACL
    pub i_size_high: u32,
    /// Obsolete fragment address
    pub i_obso_faddr: u32,
    /// OS-specific value 2 (12 bytes)
    pub i_osd2: [u8; 12],
    /// Extra inode size (beyond 128 bytes)
    pub i_extra_isize: u16,
    /// Checksum (upper 16 bits)
    pub i_checksum_hi: u16,
    /// Extra change time (high precision)
    pub i_ctime_extra: u32,
    /// Extra modification time (high precision)
    pub i_mtime_extra: u32,
    /// Extra access time (high precision)
    pub i_atime_extra: u32,
    /// Creation time (seconds since epoch)
    pub i_crtime: u32,
    /// Extra creation time (high precision)
    pub i_crtime_extra: u32,
    /// Version (high 32 bits)
    pub i_version_hi: u32,
    /// Project ID
    pub i_projid: u32,
    /// Padding to 256 bytes
    pub i_reserved: [u8; 96],
}

impl Default for ExtInode {
    fn default() -> Self {
        Self {
            i_mode: 0,
            i_uid: EXT_DEFAULT_UID,
            i_size_lo: 0,
            i_atime: 0,
            i_ctime: 0,
            i_mtime: 0,
            i_dtime: 0,
            i_gid: EXT_DEFAULT_GID,
            i_links_count: 0,
            i_blocks_lo: 0,
            i_flags: EXT_INODE_FLAG_EXTENTS,
            i_osd1: 0,
            i_block: [0; 60],
            i_generation: 0,
            i_file_acl_lo: 0,
            i_size_high: 0,
            i_obso_faddr: 0,
            i_osd2: [0; 12],
            i_extra_isize: 32, // Extended inode fields size
            i_checksum_hi: 0,
            i_ctime_extra: 0,
            i_mtime_extra: 0,
            i_atime_extra: 0,
            i_crtime: 0,
            i_crtime_extra: 0,
            i_version_hi: 0,
            i_projid: 0,
            i_reserved: [0; 96],
        }
    }
}

impl ExtInode {
    /// Create a new inode for a directory
    pub fn new_dir(mode: u16, links: u16, block: u32, extent: ExtExtent) -> Self {
        let mut inode = Self {
            i_mode: mode,
            i_links_count: links,
            i_blocks_lo: block, // Already in 512-byte units
            i_size_lo: 4096,    // One block for directory
            ..Default::default()
        };
        inode.set_extent(extent);
        inode
    }

    /// Create a new inode for a regular file
    pub fn new_file(mode: u16, size: u32, blocks: u32, extent: ExtExtent) -> Self {
        let mut inode = Self {
            i_mode: mode,
            i_links_count: 1,
            i_blocks_lo: blocks, // Already in 512-byte units
            i_size_lo: size,
            ..Default::default()
        };
        inode.set_extent(extent);
        inode
    }

    /// Set the extent header and first extent in i_block
    pub fn set_extent(&mut self, extent: ExtExtent) {
        let header = ExtExtentHeader {
            eh_entries: 1,
            ..Default::default()
        };

        // Write header (bytes 0-11)
        self.i_block[0..12].copy_from_slice(header.as_bytes());

        // Write extent (bytes 12-23)
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
            eh_entries: count,
            ..Default::default()
        };

        // Write header
        self.i_block[0..12].copy_from_slice(header.as_bytes());

        // Write extents
        for (i, extent) in extents.iter().enumerate() {
            let offset = 12 + i * 12;
            self.i_block[offset..offset + 12].copy_from_slice(extent.as_bytes());
        }
    }

    /// Set block map (Direct/Indirect)
    pub fn set_block_map(&mut self, map: &BlockMapArray) {
        self.i_block[0..60].copy_from_slice(map.as_bytes());
        // BlockMap does not use EXT_INODE_FLAG_EXTENTS, so we must clear it.
        self.i_flags &= !EXT_INODE_FLAG_EXTENTS;
    }

    /// Set timestamps
    pub fn set_timestamps(&mut self, atime: u32, ctime: u32, mtime: u32) {
        self.i_atime = atime;
        self.i_ctime = ctime;
        self.i_mtime = mtime;
    }

    /// Check if this is a directory
    pub fn is_dir(&self) -> bool {
        (self.i_mode & 0xF000) == 0x4000
    }

    /// Check if this is a regular file
    pub fn is_file(&self) -> bool {
        (self.i_mode & 0xF000) == 0x8000
    }

    /// Check if this is a symbolic link
    pub fn is_symlink(&self) -> bool {
        (self.i_mode & 0xF000) == 0xA000
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
            i_mode,
            i_uid,
            i_gid,
            i_size_lo: (size & 0xFFFF_FFFF) as u32,
            i_size_high: size_high,
            i_links_count: links,
            i_blocks_lo: blocks, // Already in 512-byte units from caller
            i_atime: atime,
            i_ctime: ctime,
            i_mtime: mtime,
            i_osd2: osd2,
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
            i_mode,
            i_uid,
            i_gid,
            i_size_lo: len as u32,
            i_size_high: 0,
            i_links_count: 1,
            i_blocks_lo: 0, // MUST be 0 blocks for fast symlink
            i_flags: 0,     // MUST NOT set EXT_INODE_FLAG_EXTENTS
            i_block,
            i_atime: atime,
            i_ctime: ctime,
            i_mtime: mtime,
            i_osd2: osd2,
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
            i_mode,
            i_uid,
            i_gid,
            i_size_lo: (size & 0xFFFF_FFFF) as u32,
            i_size_high: size_high,
            i_links_count: links,
            i_blocks_lo: blocks,
            i_atime: atime,
            i_ctime: ctime,
            i_mtime: mtime,
            i_osd2: osd2,
            ..Default::default()
        };
        inode.set_block_map(map);

        inode
    }

    /// Encode to raw bytes (256 bytes)
    pub fn to_bytes(&self) -> [u8; EXT_DEFAULT_INODE_SIZE as usize] {
        // Safe: ExtInode is exactly EXT_DEFAULT_INODE_SIZE bytes by layout and static assert
        *zerocopy::IntoBytes::as_bytes(self)
            .first_chunk()
            .expect("ExtInode size mismatch")
    }
}

// Ensure the struct is exactly 256 bytes
const _: () = assert!(core::mem::size_of::<ExtInode>() == EXT_DEFAULT_INODE_SIZE as usize);
