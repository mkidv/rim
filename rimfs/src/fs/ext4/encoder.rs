// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::{fs::ext4::constant::*, core::parser::attr::FileAttributes};

pub struct Ext4Encoder {}

impl Ext4Encoder {
    /// Encode an inode (returns full 256 bytes)
    /// Encode inode with full params (used by Formater)
    pub fn encode_inode(
        i_mode: u16,
        i_size_lo: u32,
        i_links_count: u16,
        i_blocks_lo: u32,
        i_block_0: u32,
        i_atime: u32,
        i_ctime: u32,
        i_mtime: u32,
    ) -> [u8; EXT4_DEFAULT_INODE_SIZE as usize] {
        let mut buf = [0u8; EXT4_DEFAULT_INODE_SIZE as usize];

        // i_mode
        buf[0..2].copy_from_slice(&i_mode.to_le_bytes());

        // i_size_lo
        buf[4..8].copy_from_slice(&i_size_lo.to_le_bytes());

        // Timestamps
        buf[8..12].copy_from_slice(&i_atime.to_le_bytes());
        buf[12..16].copy_from_slice(&i_ctime.to_le_bytes());
        buf[16..20].copy_from_slice(&i_mtime.to_le_bytes());

        // i_dtime → 0
        buf[20..24].copy_from_slice(&0u32.to_le_bytes());

        // i_links_count
        buf[26..28].copy_from_slice(&i_links_count.to_le_bytes());

        // i_blocks_lo
        buf[28..32].copy_from_slice(&i_blocks_lo.to_le_bytes());

        // i_flags → extents
        buf[32..36].copy_from_slice(&EXT4_INODE_FLAG_EXTENTS.to_le_bytes());

        // Write extent header
        buf[40..42].copy_from_slice(&0xf30au16.to_le_bytes()); // eh_magic
        buf[42..44].copy_from_slice(&1u16.to_le_bytes()); // eh_entries
        buf[44..46].copy_from_slice(&4u16.to_le_bytes()); // eh_max
        buf[46..48].copy_from_slice(&0u16.to_le_bytes()); // eh_depth = 0 (leaf)
        buf[48..52].copy_from_slice(&0u32.to_le_bytes()); // eh_generation

        // Write extent entry
        buf[52..56].copy_from_slice(&0u32.to_le_bytes()); // ee_block = logical block 0
        buf[56..58].copy_from_slice(&1u16.to_le_bytes()); // ee_len = 1 block
        buf[58..60].copy_from_slice(&0u16.to_le_bytes()); // ee_start_hi = 0 (car i_block_0 est u32)
        buf[60..64].copy_from_slice(&i_block_0.to_le_bytes()); // ee_start_lo = i_block_0

        buf
    }

    /// Encode inode from FileAttributes (used by Injector) → no i_mode param!
    pub fn encode_inode_from_attr(
        attr: &FileAttributes,
        i_size_lo: u32,
        i_links_count: u16,
        i_blocks_lo: u32,
        i_block_0: u32,
    ) -> [u8; EXT4_DEFAULT_INODE_SIZE as usize] {
        let i_mode = Self::inode_mode(attr);

        let atime = attr
            .accessed
            .unwrap_or_else(time::OffsetDateTime::now_utc)
            .unix_timestamp() as u32;
        let ctime = attr
            .created
            .unwrap_or_else(time::OffsetDateTime::now_utc)
            .unix_timestamp() as u32;
        let mtime = attr
            .modified
            .unwrap_or_else(time::OffsetDateTime::now_utc)
            .unix_timestamp() as u32;

        Self::encode_inode(
            i_mode,
            i_size_lo,
            i_links_count,
            i_blocks_lo,
            i_block_0,
            atime,
            ctime,
            mtime,
        )
    }

    /// Encode a directory entry (EXT4 dir_entry)
    pub fn dir_entry(inode: u32, name: &str, file_type: u8) -> Vec<u8> {
        let mut entry = vec![];

        // inode
        entry.extend_from_slice(&inode.to_le_bytes());

        // rec_len = 8 + name_len padded to 4
        let name_len = name.len();
        let rec_len = (8 + name_len).div_ceil(4) * 4;
        entry.extend_from_slice(&(rec_len as u16).to_le_bytes());

        // name_len
        entry.push(name_len as u8);

        // file_type
        entry.push(file_type);

        // name
        entry.extend_from_slice(name.as_bytes());

        // pad to 4 bytes
        while entry.len() % 4 != 0 {
            entry.push(0);
        }

        entry
    }

    pub fn dot_entry(current_inode: u32) -> Vec<u8> {
        Self::dir_entry(current_inode, ".", 2) // 2 = directory
    }

    pub fn dotdot_entry(parent_inode: u32) -> Vec<u8> {
        Self::dir_entry(parent_inode, "..", 2) // 2 = directory
    }

    pub fn file_entry_from_attr(name: &str, attr: &FileAttributes, inode: u32) -> Vec<u8> {
        let file_type = attr.as_ext4_file_type();
        Self::dir_entry(inode, name, file_type)
    }

    pub fn dir_entry_from_attr(name: &str, attr: &FileAttributes, inode: u32) -> Vec<u8> {
        let file_type = attr.as_ext4_file_type();
        Self::dir_entry(inode, name, file_type)
    }

    pub fn deleted_entry() -> Vec<u8> {
        Self::dir_entry(0, "", 0)
    }

    pub fn empty_entry() -> Vec<u8> {
        let mut entry = vec![];
        entry.extend_from_slice(&0u32.to_le_bytes());
        entry.extend_from_slice(&8u16.to_le_bytes());
        entry.push(0); // name_len
        entry.push(0); // file_type
        entry
    }

    /// Compute i_mode for inode (type + perms)
    pub fn inode_mode(attr: &FileAttributes) -> u16 {
        attr.as_ext4_mode().bits()
    }
}
