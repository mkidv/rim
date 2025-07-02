// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use crate::fs::ext4::utils::*;
use crate::fs::ext4::{Ext4Params, constant::*};
use std::collections::HashSet;
use std::convert::TryInto;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub struct Ext4Checker {
    reader: File,
    partition_offset: u64,
    pub block_size: u32,
    pub group_count: u32,
    pub blocks_per_group: u32,
    pub inodes_per_group: u32,
    pub inode_count: u32,
    pub size_bytes: u64,
}

impl Ext4Checker {
    pub fn open(path: &str, partition_offset: u64) -> std::io::Result<Self> {
        let mut reader = File::open(path)?;

        let sb_offset = partition_offset + EXT4_SUPERBLOCK_OFFSET;
        let size_bytes = reader.metadata()?.len();
        reader.seek(SeekFrom::Start(sb_offset))?;
        let mut sb_buf = [0u8; EXT4_SUPERBLOCK_SIZE];
        reader.read_exact(&mut sb_buf)?;

        let log_block_size = u32::from_le_bytes(sb_buf[0x18..0x1C].try_into().unwrap());
        let block_size = 1024 << log_block_size;

        let group_count = {
            let block_count = u32::from_le_bytes(sb_buf[0x04..0x08].try_into().unwrap());
            let blocks_per_group = u32::from_le_bytes(sb_buf[0x20..0x24].try_into().unwrap());
            (block_count + blocks_per_group - 1) / blocks_per_group
        };

        let blocks_per_group = u32::from_le_bytes(sb_buf[0x20..0x24].try_into().unwrap());
        let inodes_per_group = u32::from_le_bytes(sb_buf[0x28..0x2C].try_into().unwrap());
        let inode_count = group_count * inodes_per_group;

        println!(
            "[check] Superblock parsed OK → block_size = {block_size}, group_count = {group_count}"
        );

        Ok(Self {
            reader,
            partition_offset,
            block_size,
            group_count,
            blocks_per_group,
            inodes_per_group,
            inode_count,
            size_bytes,
        })
    }

    pub fn check_superblock(&mut self) -> Result<(), String> {
        let sb_offset = self.partition_offset + EXT4_SUPERBLOCK_OFFSET;
        self.reader.seek(SeekFrom::Start(sb_offset)).unwrap();
        let mut sb_buf = [0u8; EXT4_SUPERBLOCK_SIZE];
        self.reader.read_exact(&mut sb_buf).unwrap();

        let magic = u16::from_le_bytes(sb_buf[0x38..0x3A].try_into().unwrap());
        if magic != EXT4_SUPERBLOCK_MAGIC {
            return Err(format!("Invalid superblock magic: 0x{:04X}", magic));
        }

        println!("[check] Superblock OK");
        Ok(())
    }

    pub fn check_bgdt(&mut self, group: u32) -> Result<(), String> {
        let bgdt_offset = self.partition_offset
            + ((EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * self.block_size as u64);
        self.reader.seek(SeekFrom::Start(bgdt_offset)).unwrap();

        let mut bgdt_buf = vec![0u8; (self.group_count as usize) * EXT4_BGDT_ENTRY_SIZE];
        self.reader.read_exact(&mut bgdt_buf).unwrap();

        let group_offset = group as usize * EXT4_BGDT_ENTRY_SIZE;
        let block_bitmap_lo =
            u32::from_le_bytes(bgdt_buf[group_offset..group_offset + 4].try_into().unwrap());
        let inode_bitmap_lo = u32::from_le_bytes(
            bgdt_buf[group_offset + 4..group_offset + 8]
                .try_into()
                .unwrap(),
        );
        let inode_table_lo = u32::from_le_bytes(
            bgdt_buf[group_offset + 8..group_offset + 12]
                .try_into()
                .unwrap(),
        );

        let group_start = group * self.blocks_per_group;
        let group_end = group_start + self.blocks_per_group;

        if !(block_bitmap_lo >= group_start && block_bitmap_lo < group_end) {
            return Err(format!(
                "Group {group}: block_bitmap_lo out of range: {block_bitmap_lo}"
            ));
        }
        if !(inode_bitmap_lo >= group_start && inode_bitmap_lo < group_end) {
            return Err(format!(
                "Group {group}: inode_bitmap_lo out of range: {inode_bitmap_lo}"
            ));
        }

        let inode_table_blocks =
            (self.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / self.block_size).div_ceil(1);
        if !(inode_table_lo >= group_start && (inode_table_lo + inode_table_blocks) <= group_end) {
            return Err(format!(
                "Group {group}: inode_table_lo out of range or exceeds group: {inode_table_lo} (+{inode_table_blocks})"
            ));
        }

        println!("[check] BGDT group {group} OK");
        Ok(())
    }

    pub fn check_block_bitmap_overlap(&mut self, group: u32) -> Result<(), String> {
        // Read BGDT
        let bgdt_offset = self.partition_offset
            + ((EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * self.block_size as u64);
        self.reader.seek(SeekFrom::Start(bgdt_offset)).unwrap();

        let mut bgdt_buf = vec![0u8; (self.group_count as usize) * EXT4_BGDT_ENTRY_SIZE];
        self.reader.read_exact(&mut bgdt_buf).unwrap();

        let group_offset = group as usize * EXT4_BGDT_ENTRY_SIZE;
        let block_bitmap_lo =
            u32::from_le_bytes(bgdt_buf[group_offset..group_offset + 4].try_into().unwrap());
        let inode_bitmap_lo = u32::from_le_bytes(
            bgdt_buf[group_offset + 4..group_offset + 8]
                .try_into()
                .unwrap(),
        );
        let inode_table_lo = u32::from_le_bytes(
            bgdt_buf[group_offset + 8..group_offset + 12]
                .try_into()
                .unwrap(),
        );
        let inode_table_blocks =
            (self.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / self.block_size).div_ceil(1);

        let group_start = group * self.blocks_per_group;
        let reserved_blocks = reserved_blocks_in_group(
            group,
            &Ext4Params {
                block_count: self.group_count * self.blocks_per_group,
                blocks_per_group: self.blocks_per_group,
                inodes_per_group: self.inodes_per_group,
                block_size: self.block_size,
                first_data_block: 0, // <- pas utile ici car utilisé indirectement par reserved_blocks_in_group
                volume_id: [0u8; 16],
                volume_label: String::new(),
                group_count: self.group_count,
                inode_count: self.inode_count,
                size_bytes: self.size_bytes,
            },
        );

        let mut expected_used_bits = HashSet::new();

        // Reserved blocks
        for i in 0..reserved_blocks {
            expected_used_bits.insert(i);
        }

        // block_bitmap block
        expected_used_bits.insert(block_bitmap_lo - group_start);

        // inode_bitmap block
        expected_used_bits.insert(inode_bitmap_lo - group_start);

        // inode_table blocks
        for i in 0..inode_table_blocks {
            expected_used_bits.insert(inode_table_lo - group_start + i);
        }

        // root dir block → group 0 only
        if group == 0 {
            let root_dir_block = first_data_block_in_group(
                &Ext4Params {
                    block_count: self.group_count * self.blocks_per_group,
                    blocks_per_group: self.blocks_per_group,
                    inodes_per_group: self.inodes_per_group,
                    block_size: self.block_size,
                    first_data_block: 0,
                    volume_id: [0u8; 16],
                    volume_label: String::new(),
                    group_count: self.group_count,
                    inode_count: self.inode_count,
                    size_bytes: self.size_bytes,
                },
                group,
            );
            expected_used_bits.insert(root_dir_block - group_start);
        }

        // Read block_bitmap
        let block_bitmap_offset =
            self.partition_offset + (block_bitmap_lo as u64 * self.block_size as u64);
        self.reader
            .seek(SeekFrom::Start(block_bitmap_offset))
            .unwrap();

        let bitmap_size = (self.blocks_per_group / 8) as usize;
        let mut block_bitmap = vec![0u8; bitmap_size];
        self.reader.read_exact(&mut block_bitmap).unwrap();

        // Check all bits
        let mut errors = vec![];

        for i in 0..self.blocks_per_group as usize {
            let byte = i / 8;
            let bit_in_byte = i % 8;
            let is_set = (block_bitmap[byte] & (1 << bit_in_byte)) != 0;
            let should_be_set = expected_used_bits.contains(&(i as u32));

            if is_set && !should_be_set {
                errors.push(i);
            }
        }

        if errors.is_empty() {
            println!("[check] Block bitmap group {group} overlap OK");
            Ok(())
        } else {
            println!(
                "[check] Block bitmap group {group} overlap ERROR → bits {errors:?} should be 0"
            );
            Err(format!("Block bitmap group {group} overlap ERROR"))
        }
    }

    pub fn check_inode_bitmap(&mut self, group: u32) -> Result<(), String> {
        let bgdt_offset = self.partition_offset
            + ((EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * self.block_size as u64);
        self.reader.seek(SeekFrom::Start(bgdt_offset)).unwrap();

        let mut bgdt_buf = vec![0u8; (self.group_count as usize) * EXT4_BGDT_ENTRY_SIZE];
        self.reader.read_exact(&mut bgdt_buf).unwrap();

        let group_offset = group as usize * EXT4_BGDT_ENTRY_SIZE;
        let inode_bitmap_lo = u32::from_le_bytes(
            bgdt_buf[group_offset + 4..group_offset + 8]
                .try_into()
                .unwrap(),
        );

        let inode_bitmap_offset =
            self.partition_offset + (inode_bitmap_lo as u64 * self.block_size as u64);
        self.reader
            .seek(SeekFrom::Start(inode_bitmap_offset))
            .unwrap();

        let inode_bitmap_size = (self.inodes_per_group / 8) as usize;
        let mut inode_bitmap = vec![0u8; inode_bitmap_size];
        self.reader.read_exact(&mut inode_bitmap).unwrap();

        let valid_bits = self.inodes_per_group as usize;
        for i in valid_bits..(inode_bitmap_size * 8) {
            let byte = i / 8;
            let bit_in_byte = i % 8;
            if (inode_bitmap[byte] & (1 << bit_in_byte)) != 0 {
                return Err(format!(
                    "Group {group}: inode_bitmap has bit {} set out of range",
                    i
                ));
            }
        }

        println!("[check] Inode bitmap group {group} OK");
        Ok(())
    }

    pub fn check_block_bitmap_position(&mut self, group: u32) -> Result<(), String> {
        // Read BGDT
        let bgdt_offset = self.partition_offset
            + ((EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * self.block_size as u64);
        self.reader.seek(SeekFrom::Start(bgdt_offset)).unwrap();

        let mut bgdt_buf = vec![0u8; (self.group_count as usize) * EXT4_BGDT_ENTRY_SIZE];
        self.reader.read_exact(&mut bgdt_buf).unwrap();

        let group_offset = group as usize * EXT4_BGDT_ENTRY_SIZE;
        let block_bitmap_lo =
            u32::from_le_bytes(bgdt_buf[group_offset..group_offset + 4].try_into().unwrap());

        let group_start = group * self.blocks_per_group;

        let reserved_blocks = reserved_blocks_in_group(
            group,
            &Ext4Params {
                block_count: self.group_count * self.blocks_per_group,
                blocks_per_group: self.blocks_per_group,
                inodes_per_group: self.inodes_per_group,
                block_size: self.block_size,
                first_data_block: 0,
                volume_id: [0u8; 16],
                volume_label: String::new(),
                group_count: self.group_count,
                inode_count: self.inode_count,
                size_bytes: self.size_bytes,
            },
        );

        let required_start = group_start + reserved_blocks;

        if block_bitmap_lo < required_start {
            println!(
                "[check] Block bitmap group {group} position ERROR → block_bitmap_lo={block_bitmap_lo}, required >= {required_start}"
            );
            return Err(format!("Block bitmap group {group} position ERROR"));
        } else {
            println!(
                "[check] Block bitmap group {group} position OK → block_bitmap_lo={block_bitmap_lo} >= reserved_end={required_start}"
            );
            Ok(())
        }
    }

    pub fn check_all(&mut self) -> Result<(), String> {
        self.check_superblock()?;
        for group in 0..self.group_count {
            self.check_bgdt(group)?;
            self.check_block_bitmap_overlap(group)?;
            self.check_block_bitmap_position(group)?;
            self.check_inode_bitmap(group)?;
        }
        println!("[check] All checks passed.");
        Ok(())
    }
}
