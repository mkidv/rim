// SPDX-License-Identifier: MIT
// rimgen/fs/ext4/formater.rs

use crate::fs::ext4::encoder::Ext4Encoder;
use crate::fs::ext4::utils::*;
use crate::fs::ext4::{constant::*, params::Ext4Params};
use crate::core::attr::FileAttributes;
use crate::core::{
    FsFormatterResult, allocator::FsAllocator, formater::FsFormatter, io::FsBlockIO,
};

pub struct Ext4Formatter;

impl FsFormatter<u32, Ext4Params> for Ext4Formatter {
    fn format(
        &mut self,
        io: &mut dyn FsBlockIO<u32>,
        block_allocator: &mut dyn FsAllocator<u32>,
        params: &Ext4Params,
    ) -> FsFormatterResult {
        Self::write_superblock(io, params)?;
        Self::write_bgdt(io, params)?;
        Self::write_bitmaps(io, params)?;
        Self::write_inode_tables(io, params)?;
        // Self::write_root_dir(io, block_allocator, params)?;
        for group_id in 0..params.group_count {
            Self::log_group_layout(group_id, params);
        }
        io.flush()?;
        println!("[ext4] format done.");
        Ok(())
    }
}

impl Ext4Formatter {
    pub fn new() -> Self {
        Self {}
    }

    pub fn log_group_layout(group_id: u32, params: &Ext4Params) {
        let group_start = params.first_data_block + group_id * params.blocks_per_group;
        let group_end = group_start + params.blocks_per_group;
        let reserved_blocks = reserved_blocks_in_group(group_id, params);

        let block_bitmap_block = block_bitmap_block(group_id, params);
        let inode_bitmap_block = inode_bitmap_block(group_id, params);
        let inode_table_block = inode_table_block(group_id, params);
        let inode_table_blocks =
            (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);

        let first_data_block = first_data_block_in_group(params, group_id);

        let used_blocks = {
            let mut used = reserved_blocks;
            used += 1; // block_bitmap
            used += 1; // inode_bitmap
            used += inode_table_blocks;
            if group_id == 0 {
                used += 1; // root dir block
            }
            used
        };

        let used_inodes = if group_id == 0 { 2 } else { 0 };

        println!(
            "[ext4] Group {group_id}: block_bitmap = {block_bitmap_block}, inode_bitmap = {inode_bitmap_block}, inode_table = {inode_table_block} (+{inode_table_blocks} blocks), first_data_block = {first_data_block}, reserved_blocks = {reserved_blocks}"
        );

        println!("[ext4] Group {group_id}: range = [{group_start} .. {group_end})");

        println!(
            "[ext4] Group {group_id}: used_blocks = {}, free_blocks = {}",
            used_blocks,
            params.blocks_per_group - used_blocks
        );

        println!(
            "[ext4] Group {group_id}: used_inodes = {}, free_inodes = {}",
            used_inodes,
            params.inodes_per_group - used_inodes
        );

        // Vérif root_dir_block → super utile
        if group_id == 0 {
            let root_dir_block = first_data_block_in_group(params, group_id);
            println!(
                "[ext4] Group {group_id}: root_dir_block = {root_dir_block}, expected bitmap bit = {}",
                (root_dir_block - group_start)
            );
        }

        assert!(
            block_bitmap_block >= group_start && block_bitmap_block < group_end,
            "ERROR: Block bitmap for group {group_id} is out of group!"
        );
        assert!(
            inode_bitmap_block >= group_start && inode_bitmap_block < group_end,
            "ERROR: Inode bitmap for group {group_id} is out of group!"
        );
        assert!(
            inode_table_block >= group_start
                && (inode_table_block + inode_table_blocks) <= group_end,
            "ERROR: Inode table for group {group_id} exceeds group!"
        );
        assert!(
            first_data_block >= group_start && first_data_block < group_end,
            "ERROR: First data block for group {group_id} is out of group!"
        );
    }

    fn write_superblock(io: &mut dyn FsBlockIO<u32>, params: &Ext4Params) -> FsFormatterResult {
        println!("[ext4] Writing superblock...");

        let mut buf = [0u8; EXT4_SUPERBLOCK_SIZE];

        // === Blocks used ===
        let mut used_blocks: u32 = 0;

        for group in 0..params.group_count {
            used_blocks += 1; // block_bitmap
            used_blocks += 1; // inode_bitmap

            let inode_table_blocks =
                (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);
            used_blocks += inode_table_blocks;

            // root dir block → uniquement pour group 0
            if group == 0 {
                used_blocks += 1;
            }
        }

        println!(
            "[ext4] Superblock: used_blocks = {}, free_blocks = {}",
            used_blocks,
            params.block_count - used_blocks
        );

        // === Inodes used ===
        let used_inodes: u32 = EXT4_ROOT_INODE; // inode 1 (bad blocks) + inode 2 (root dir)

        // === Fill superblock ===

        // Magic
        buf[0x38..0x3A].copy_from_slice(&EXT4_SUPERBLOCK_MAGIC.to_le_bytes());

        // Basic fields
        buf[0x00..0x04].copy_from_slice(&params.inode_count.to_le_bytes());
        buf[0x04..0x08].copy_from_slice(&params.block_count.to_le_bytes());
        buf[0x08..0x0C].copy_from_slice(&0u32.to_le_bytes()); // reserved blocks

        let free_blocks = params.block_count - used_blocks;
        buf[0x0C..0x10].copy_from_slice(&free_blocks.to_le_bytes());

        let free_inodes = params.inode_count - used_inodes;
        buf[0x10..0x14].copy_from_slice(&free_inodes.to_le_bytes());

        buf[0x14..0x18].copy_from_slice(&params.first_data_block.to_le_bytes());

        let log_block_size = params.block_size.trailing_zeros() - 10;
        buf[0x18..0x1C].copy_from_slice(&log_block_size.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&log_block_size.to_le_bytes());

        buf[0x20..0x24].copy_from_slice(&params.blocks_per_group.to_le_bytes());
        buf[0x24..0x28].copy_from_slice(&params.blocks_per_group.to_le_bytes());
        buf[0x28..0x2C].copy_from_slice(&params.inodes_per_group.to_le_bytes());

        buf[0x2C..0x30].copy_from_slice(&0u32.to_le_bytes()); // mount time
        buf[0x30..0x34].copy_from_slice(&0u32.to_le_bytes()); // write time
        buf[0x34..0x36].copy_from_slice(&0u16.to_le_bytes()); // mount count
        buf[0x36..0x38].copy_from_slice(&0xFFFFu16.to_le_bytes()); // max mount count

        // Redundant magic for safety (OK dans spec)
        buf[0x38..0x3A].copy_from_slice(&EXT4_SUPERBLOCK_MAGIC.to_le_bytes());
        buf[0x3A..0x3C].copy_from_slice(&1u16.to_le_bytes()); // fs_state = clean
        buf[0x3C..0x3E].copy_from_slice(&1u16.to_le_bytes()); // errors = continue
        buf[0x3E..0x40].copy_from_slice(&0u16.to_le_bytes()); // minor_rev_level

        buf[0x40..0x44].copy_from_slice(&0u32.to_le_bytes()); // lastcheck
        buf[0x44..0x48].copy_from_slice(&0u32.to_le_bytes()); // checkinterval

        buf[0x48..0x4C].copy_from_slice(&0u32.to_le_bytes()); // creator OS → Linux = 0
        buf[0x4C..0x50].copy_from_slice(&1u32.to_le_bytes()); // rev_level = dynamic
        buf[0x50..0x54].copy_from_slice(&0u32.to_le_bytes()); // def_resuid
        buf[0x54..0x58].copy_from_slice(&(EXT4_FIRST_INODE).to_le_bytes()); // first_ino

        buf[0x58..0x5A].copy_from_slice(&(EXT4_DEFAULT_INODE_SIZE as u16).to_le_bytes());

        // Features
        let feature_compat = EXT4_FEATURE_COMPAT_EXT_ATTR | EXT4_FEATURE_COMPAT_DIR_INDEX;
        buf[0x5C..0x60].copy_from_slice(&feature_compat.to_le_bytes());

        let feature_incompat = EXT4_FEATURE_INCOMPAT_EXTENTS;
        buf[0x60..0x64].copy_from_slice(&feature_incompat.to_le_bytes());

        let feature_ro_compat = EXT4_FEATURE_RO_COMPAT_SPARSE_SUPER
            | EXT4_FEATURE_RO_COMPAT_LARGE_FILE
            | EXT4_FEATURE_RO_COMPAT_DIR_NLINK
            | EXT4_FEATURE_RO_COMPAT_EXTRA_ISIZE;
        buf[0x64..0x68].copy_from_slice(&feature_ro_compat.to_le_bytes());

        // UUID
        buf[0x68..0x78].copy_from_slice(&params.volume_id);

        // Volume name
        let mut label_bytes = [0u8; 16];
        let label = params.volume_label.as_bytes();
        label_bytes[..label.len().min(16)].copy_from_slice(&label[..label.len().min(16)]);
        buf[0x78..0x88].copy_from_slice(&label_bytes);

        // Remaining fields → safe defaults
        buf[0x88..0xC8].fill(0);

        // Log superblock features
        println!(
            "[ext4] Superblock features -> compat: 0x{feature_compat:08X}, incompat: 0x{feature_incompat:08X}, ro_compat: 0x{feature_ro_compat:08X}"
        );

        // === Write primary superblock
        let offset = EXT4_SUPERBLOCK_OFFSET;
        io.write_at(offset, &buf)?;

        // === Write sparse superblocks ===
        for group_id in 0..params.group_count {
            if is_sparse_super_group(group_id) && group_id != 0 {
                println!("[ext4] Writing sparse superblock copy for group {group_id}");
                let group_start_block =
                    params.first_data_block + group_id * params.blocks_per_group;
                let sb_copy_offset = (group_start_block * params.block_size) as u64;
                write_sparse_superblock_entry(io, sb_copy_offset, &buf)?;
            }
        }

        Ok(())
    }

    fn write_bgdt(io: &mut dyn FsBlockIO<u32>, params: &Ext4Params) -> FsFormatterResult {
        println!("[ext4] Writing Block Group Descriptor Table...");

        let mut buf = vec![0u8; EXT4_BGDT_ENTRY_SIZE * params.group_count as usize];

        for group in 0..params.group_count as usize {
            let block_bitmap_lo = block_bitmap_block(group as u32, params);
            let inode_bitmap_lo = inode_bitmap_block(group as u32, params);
            let inode_table_lo = inode_table_block(group as u32, params);

            println!(
                "[ext4] BGDT group {group}: block_bitmap_lo = {block_bitmap_lo}, inode_bitmap_lo = {inode_bitmap_lo}, inode_table_lo = {inode_table_lo}"
            );

            let group_offset = group * EXT4_BGDT_ENTRY_SIZE;
            buf[group_offset..group_offset + 4].copy_from_slice(&block_bitmap_lo.to_le_bytes());
            buf[group_offset + 4..group_offset + 8].copy_from_slice(&inode_bitmap_lo.to_le_bytes());
            buf[group_offset + 8..group_offset + 12].copy_from_slice(&inode_table_lo.to_le_bytes());

            // === Compute used/free blocks/inodes in this group ===
            let used = compute_used_blocks_in_group(group as u32, params);
            let free_blocks = params.blocks_per_group - used;

            let free_inodes = params.inodes_per_group - compute_used_inodes_in_group(group as u32);

            buf[group_offset + 12..group_offset + 14]
                .copy_from_slice(&(free_blocks as u16).to_le_bytes());
            buf[group_offset + 14..group_offset + 16]
                .copy_from_slice(&(free_inodes as u16).to_le_bytes());
            buf[group_offset + 16..group_offset + 18].copy_from_slice(&0u16.to_le_bytes()); // used_dirs_count
            buf[group_offset + 18..group_offset + 20].copy_from_slice(&0u16.to_le_bytes()); // pad

            buf[group_offset + 20..group_offset + EXT4_BGDT_ENTRY_SIZE].fill(0);
        }

        let offset = (EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * params.block_size as u64;
        io.write_at(offset, &buf)?;

        // === Write sparse ===
        for group_id in 0..params.group_count {
            if is_sparse_super_group(group_id) && group_id != 0 {
                println!("[ext4] Writing sparse BGDT copy for group {group_id}");
                let group_start_block =
                    params.first_data_block + group_id * params.blocks_per_group;
                let sb_copy_offset = (group_start_block * params.block_size) as u64;
                let bgdt_copy_offset = sb_copy_offset + params.block_size as u64;
                write_sparse_bgdt_entry(io, bgdt_copy_offset, group_id, &buf)?;
            }
        }

        Ok(())
    }

    fn write_bitmaps(io: &mut dyn FsBlockIO<u32>, params: &Ext4Params) -> FsFormatterResult {
        println!("[ext4] Writing Block + Inode bitmaps...");

        let bitmap_size = (params.blocks_per_group / 8) as usize;
        let inode_bitmap_size = (params.inodes_per_group / 8) as usize;

        for group in 0..params.group_count as usize {
            // Block bitmap
            let mut block_bitmap = vec![0u8; bitmap_size];
            let block_bitmap_block = block_bitmap_block(group as u32, params);
            let inode_bitmap_block = inode_bitmap_block(group as u32, params);
            let inode_table_block = inode_table_block(group as u32, params);

            let group_start = params.first_data_block + group as u32 * params.blocks_per_group;

            let reserved_blocks = reserved_blocks_in_group(group as u32, params);
            for i in 0..reserved_blocks {
                block_bitmap.set_bit(i as usize, true);
            }

            // block_bitmap block
            block_bitmap.set_bit((block_bitmap_block - group_start) as usize, true);
            block_bitmap.set_bit((inode_bitmap_block - group_start) as usize, true);

            let table_blocks =
                (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);

            for i in 0..table_blocks {
                block_bitmap.set_bit((inode_table_block - group_start + i) as usize, true);
            }

            if group == 0 {
                block_bitmap.set_bit(
                    (first_data_block_in_group(params, group as u32) - group_start) as usize,
                    true,
                );
            }

            // Zero high bits at end of block_bitmap:
            let valid_bits = params.blocks_per_group as usize;
            for i in valid_bits..(block_bitmap.len() * 8) {
                block_bitmap.set_bit(i, false);
            }

            let block_offset = params.block_size as u64 * block_bitmap_block as u64;
            io.write_at(block_offset, &block_bitmap)?;

            // print_bitmap_bits_set_compact(
            //     &format!("[ext4] Group {group} block_bitmap"),
            //     &block_bitmap,
            // );
            // Inode bitmap
            let mut inode_bitmap = vec![0u8; inode_bitmap_size];

            if group == 0 {
                inode_bitmap.set_bit((EXT4_ROOT_INODE - 1) as usize, true); // EXT4_ROOT_INODE = 2 donc bit 1
            }

            // Zero high bits at end of inode_bitmap:
            let valid_inodes = params.inodes_per_group as usize;
            for i in valid_inodes..(inode_bitmap.len() * 8) {
                inode_bitmap.set_bit(i, false);
            }

            // Prepare full block aligned inode_bitmap block
            let mut full_inode_bitmap_block = vec![0u8; params.block_size as usize];
            full_inode_bitmap_block[..inode_bitmap.len()].copy_from_slice(&inode_bitmap);

            // Write full aligned inode_bitmap block
            let inode_offset = params.block_size as u64 * inode_bitmap_block as u64;

            io.write_at(inode_offset, &full_inode_bitmap_block)?;

            // print_bitmap_bits_set_compact(
            //     &format!("[ext4] Group {group} inode_bitmap"),
            //     &inode_bitmap,
            // );
        }

        Ok(())
    }

    fn write_inode_tables(io: &mut dyn FsBlockIO<u32>, params: &Ext4Params) -> FsFormatterResult {
        println!("[ext4] Writing Inode tables...");

        let inode_table_size = (EXT4_DEFAULT_INODE_SIZE * params.inodes_per_group) as usize;

        for group in 0..params.group_count as usize {
            let inode_table_block = inode_table_block(group as u32, params);
            let offset = inode_table_block as u64 * params.block_size as u64;

            // Just write empty inode table
            let buf = vec![0u8; inode_table_size];
            io.write_at(offset, &buf)?;
        }

        Ok(())
    }

    fn write_root_dir(
        io: &mut dyn FsBlockIO<u32>,
        block_allocator: &mut dyn FsAllocator<u32>,
        params: &Ext4Params,
    ) -> FsFormatterResult {
        println!("[ext4] Writing root directory...");

        // Allocate first free block for root dir
        let root_dir_block = block_allocator.allocate_block(); // block number

        // Prepare root dir block content
        let mut dir_buf = vec![];

        // "." entry
        dir_buf.extend_from_slice(&Ext4Encoder::dot_entry(EXT4_ROOT_INODE));
        // ".." entry
        dir_buf.extend_from_slice(&Ext4Encoder::dotdot_entry(EXT4_ROOT_INODE));

        // Pad rest of block
        while dir_buf.len() < params.block_size as usize {
            dir_buf.push(0);
        }

        // Write root dir block
        let block_offset = block_allocator.block_offset(root_dir_block);
        io.write_at(block_offset, &dir_buf)?;

        // Patch inode 2
        let root_inode_buf = Ext4Encoder::encode_inode_from_attr(
            &FileAttributes::new_folder(),
            params.block_size,
            EXT4_ROOT_DIR_LINKS_COUNT,
            params.block_size / 512,
            root_dir_block,
        );

        let inode_table_block = inode_table_block(0, params);
        let inode_table_offset = inode_table_block as u64 * params.block_size as u64;
        let inode_offset =
            inode_table_offset + (EXT4_ROOT_INODE as u64 - 1) * EXT4_DEFAULT_INODE_SIZE as u64;

        io.write_at(inode_offset, &root_inode_buf)?;
        println!("[ext4] Root directory written at block {root_dir_block}");
        Ok(())
    }
}

trait BitmapExt {
    fn set_bit(&mut self, bit: usize, value: bool);
}

impl BitmapExt for [u8] {
    fn set_bit(&mut self, bit: usize, value: bool) {
        let byte = bit / 8;
        let bit_in_byte = bit % 8;
        if byte < self.len() {
            if value {
                self[byte] |= 1 << bit_in_byte;
            } else {
                self[byte] &= !(1 << bit_in_byte);
            }
        }
    }
}

// fn print_bitmap_bits_set_compact(label: &str, bitmap: &[u8]) {
//     let mut bits = vec![];
//     for bit in 0..bitmap.len() * 8 {
//         let byte = bit / 8;
//         let bit_in_byte = bit % 8;
//         if (bitmap[byte] & (1 << bit_in_byte)) != 0 {
//             bits.push(bit);
//         }
//     }

//     let mut ranges = vec![];
//     let mut i = 0;
//     while i < bits.len() {
//         let start = bits[i];
//         let mut end = start;

//         while i + 1 < bits.len() && bits[i + 1] == end + 1 {
//             i += 1;
//             end = bits[i];
//         }

//         if start == end {
//             ranges.push(format!("{}", start));
//         } else {
//             ranges.push(format!("{}-{}", start, end));
//         }

//         i += 1;
//     }

//     println!("{label} bits set = {}", ranges.join(", "));
// }

/// Write a sparse superblock entry (1 copy of the full superblock)
fn write_sparse_superblock_entry(
    io: &mut dyn FsBlockIO<u32>,
    sb_copy_offset: u64,
    superblock_buf: &[u8],
) -> FsFormatterResult {
    println!("[ext4] Writing sparse superblock entry at offset 0x{sb_copy_offset:X}");
    io.write_at(sb_copy_offset, superblock_buf)?;
    Ok(())
}

/// Write a sparse BGDT entry (1 entry only)
fn write_sparse_bgdt_entry(
    io: &mut dyn FsBlockIO<u32>,
    bgdt_copy_offset: u64,
    group_id: u32,
    bgdt_buf: &[u8],
) -> FsFormatterResult {
    let group_offset = group_id as usize * EXT4_BGDT_ENTRY_SIZE;
    let bgdt_entry = &bgdt_buf[group_offset..group_offset + EXT4_BGDT_ENTRY_SIZE];

    let block_bitmap_lo = u32::from_le_bytes(bgdt_entry[0..4].try_into().unwrap());
    let inode_bitmap_lo = u32::from_le_bytes(bgdt_entry[4..8].try_into().unwrap());
    let inode_table_lo = u32::from_le_bytes(bgdt_entry[8..12].try_into().unwrap());

    println!(
        "[ext4] Sparse BGDT group {group_id} → block_bitmap_lo={block_bitmap_lo} inode_bitmap_lo={inode_bitmap_lo} inode_table_lo={inode_table_lo}"
    );

    io.write_at(bgdt_copy_offset, bgdt_entry)?;
    Ok(())
}
