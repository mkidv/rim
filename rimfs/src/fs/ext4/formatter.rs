// SPDX-License-Identifier: MIT

use rimio::prelude::*;
use zerocopy::IntoBytes;

use crate::core::traits::FileAttributes;
use crate::core::{FsFormatterResult, formatter::FsFormatter};
use crate::fs::ext4::types::{
    Ext4BlockGroupDesc, Ext4DirEntry, Ext4Extent, Ext4Inode, Ext4Superblock,
};
use crate::fs::ext4::utils::*;
use crate::fs::ext4::{constant::*, group_layout::GroupLayout, meta::Ext4Meta};

pub struct Ext4Formatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a Ext4Meta,
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for Ext4Formatter<'a, IO> {
    fn format(&mut self, _full_format: bool) -> FsFormatterResult {
        Self::write_superblock(self.io, self.meta)?;
        Self::write_bgdt(self.io, self.meta)?;
        Self::write_bitmaps(self.io, self.meta)?;
        Self::write_inode_tables(self.io, self.meta)?;
        Self::write_root_dir(self.io, self.meta)?;

        self.io.flush()?;
        Ok(())
    }
}

impl<'a, IO: RimIO + ?Sized> Ext4Formatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a Ext4Meta) -> Self {
        Self { io, meta }
    }

    fn write_superblock(io: &mut IO, meta: &Ext4Meta) -> FsFormatterResult {
        let mut used_blocks: u32 = 0;

        for group in 0..meta.group_count {
            used_blocks += 1; // block_bitmap
            used_blocks += 1; // inode_bitmap

            let inode_table_blocks =
                (meta.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / meta.block_size).div_ceil(1);
            used_blocks += inode_table_blocks;

            // root dir block → only for group 0
            if group == 0 {
                used_blocks += 1;
            }
        }

        // Inodes used
        let used_inodes: u32 = EXT4_ROOT_INODE; // inode 1 (bad blocks) + inode 2 (root dir)

        // Create superblock using struct
        let sb = Ext4Superblock::from_meta(meta, used_blocks, used_inodes);
        let buf = sb.to_bytes();

        // Write primary superblock
        let offset = EXT4_SUPERBLOCK_OFFSET;
        io.write_at(offset, &buf)?;

        // Write sparse superblocks
        for group_id in 0..meta.group_count {
            if is_sparse_super_group(group_id) && group_id != 0 {
                let group_start_block = meta.first_data_block + group_id * meta.blocks_per_group;
                let sb_copy_offset = (group_start_block * meta.block_size) as u64;
                io.write_at(sb_copy_offset, &buf)?;
            }
        }

        Ok(())
    }

    fn write_bgdt(io: &mut IO, meta: &Ext4Meta) -> FsFormatterResult {
        let mut buf = vec![0u8; EXT4_BGDT_ENTRY_SIZE * meta.group_count as usize];

        for group in 0..meta.group_count as usize {
            let layout = GroupLayout::compute(meta, group as u32);

            // Compute free blocks/inodes in this group
            let used = compute_used_blocks_in_group(layout, meta);
            let free_blocks = (meta.blocks_per_group - used) as u16;
            let free_inodes =
                (meta.inodes_per_group - compute_used_inodes_in_group(group as u32)) as u16;

            // Create block group descriptor using struct
            let bgd = Ext4BlockGroupDesc::new(
                layout.block_bitmap_block,
                layout.inode_bitmap_block,
                layout.inode_table_block,
                free_blocks,
                free_inodes,
            );

            let group_offset = group * EXT4_BGDT_ENTRY_SIZE;
            buf[group_offset..group_offset + EXT4_BGDT_ENTRY_SIZE].copy_from_slice(bgd.as_bytes());
        }

        let offset = (EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * meta.block_size as u64;
        io.write_at(offset, &buf)?;

        // Write sparse
        for group_id in 0..meta.group_count {
            if is_sparse_super_group(group_id) && group_id != 0 {
                let group_start_block = meta.first_data_block + group_id * meta.blocks_per_group;
                let sb_copy_offset = (group_start_block * meta.block_size) as u64;
                let bgdt_copy_offset = sb_copy_offset + meta.block_size as u64;
                let group_offset = group_id as usize * EXT4_BGDT_ENTRY_SIZE;
                let bgdt_entry = &buf[group_offset..group_offset + EXT4_BGDT_ENTRY_SIZE];

                io.write_at(bgdt_copy_offset, bgdt_entry)?;
            }
        }

        Ok(())
    }

    fn write_bitmaps(io: &mut IO, meta: &Ext4Meta) -> FsFormatterResult {
        let inode_bitmap_size = (meta.inodes_per_group / 8) as usize;

        for group in 0..meta.group_count as usize {
            // Block bitmap
            let bitmap_size = (meta.blocks_per_group / 8) as usize;
            let mut block_bitmap = vec![0u8; bitmap_size];

            let layout = GroupLayout::compute(meta, group as u32);
            let block_bitmap_block = layout.block_bitmap_block;
            let inode_bitmap_block = layout.inode_bitmap_block;
            let inode_table_block = layout.inode_table_block;

            let group_start = layout.group_start; // meta.first_data_block + group as u32 * meta.blocks_per_group;

            let reserved_blocks = layout.reserved_blocks;
            for i in 0..reserved_blocks {
                block_bitmap.set_bit(i as usize, true);
            }

            // block_bitmap block
            block_bitmap.set_bit((block_bitmap_block - group_start) as usize, true);
            block_bitmap.set_bit((inode_bitmap_block - group_start) as usize, true);

            let table_blocks =
                (meta.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / meta.block_size).div_ceil(1);

            for i in 0..table_blocks {
                block_bitmap.set_bit((inode_table_block - group_start + i) as usize, true);
            }

            if group == 0 {
                block_bitmap.set_bit((layout.first_data_block - group_start) as usize, true);
            }

            // Zero high bits at end of block_bitmap:
            let valid_bits = meta.blocks_per_group as usize;
            for i in valid_bits..(block_bitmap.len() * 8) {
                block_bitmap.set_bit(i, false);
            }

            let block_offset = meta.block_size as u64 * block_bitmap_block as u64;
            io.write_at(block_offset, &block_bitmap)?;

            // print_bitmap_bits_set_compact(
            //     &format!("[ext4] Group {group} block_bitmap"),
            //     &block_bitmap,
            // );
            // Inode bitmap
            let mut inode_bitmap = vec![0u8; inode_bitmap_size];

            if group == 0 {
                inode_bitmap.set_bit((EXT4_ROOT_INODE - 1) as usize, true); // EXT4_ROOT_INODE = 2 so bit 1
            }

            // Zero high bits at end of inode_bitmap:
            let valid_inodes = meta.inodes_per_group as usize;
            for i in valid_inodes..(inode_bitmap.len() * 8) {
                inode_bitmap.set_bit(i, false);
            }

            // Prepare full block aligned inode_bitmap block
            let mut full_inode_bitmap_block = vec![0u8; meta.block_size as usize];
            full_inode_bitmap_block[..inode_bitmap.len()].copy_from_slice(&inode_bitmap);

            // Write full aligned inode_bitmap block
            let inode_offset = meta.block_size as u64 * inode_bitmap_block as u64;

            io.write_at(inode_offset, &full_inode_bitmap_block)?;

            // print_bitmap_bits_set_compact(
            //     &format!("[ext4] Group {group} inode_bitmap"),
            //     &inode_bitmap,
            // );
        }

        Ok(())
    }

    fn write_inode_tables(io: &mut IO, meta: &Ext4Meta) -> FsFormatterResult {
        let inode_table_size = (EXT4_DEFAULT_INODE_SIZE * meta.inodes_per_group) as usize;

        for group in 0..meta.group_count {
            let layout = GroupLayout::compute(meta, group);
            let inode_table_block = layout.inode_table_block;

            let offset = inode_table_block as u64 * meta.block_size as u64;

            io.zero_fill(offset, inode_table_size)?;
        }

        Ok(())
    }

    fn write_root_dir(io: &mut IO, meta: &Ext4Meta) -> FsFormatterResult {
        // Root dir is always at the first data block of group 0
        let layout = GroupLayout::compute(meta, 0);
        let root_dir_block = layout.first_data_block;

        // Prepare root dir block content
        let mut dir_buf = vec![];

        // "." entry
        dir_buf.extend_from_slice(&Ext4DirEntry::dot(EXT4_ROOT_INODE).to_bytes());
        // ".." entry
        dir_buf.extend_from_slice(&Ext4DirEntry::dotdot(EXT4_ROOT_INODE).to_bytes());

        // Pad rest of block
        while dir_buf.len() < meta.block_size as usize {
            dir_buf.push(0);
        }

        // Write root dir block
        let block_offset = root_dir_block as u64 * meta.block_size as u64;
        io.write_at(block_offset, &dir_buf)?;

        // Patch inode 2
        let extent = Ext4Extent::new(0, root_dir_block, 1);
        let root_inode = Ext4Inode::from_attr(
            &FileAttributes::new_dir(),
            meta.block_size as u64,
            EXT4_ROOT_DIR_LINKS_COUNT,
            meta.block_size.div_ceil(512),
            &[extent],
        );
        let root_inode_buf = root_inode.to_bytes();

        let inode_table_block = layout.inode_table_block;

        let inode_table_offset = inode_table_block as u64 * meta.block_size as u64;
        let inode_offset =
            inode_table_offset + (EXT4_ROOT_INODE as u64 - 1) * EXT4_DEFAULT_INODE_SIZE as u64;

        io.write_at(inode_offset, &root_inode_buf)?;
        Ok(())
    }
}

// Use the shared BitmapOps trait from core
use crate::core::utils::bitmap::BitmapOps;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::ext4::prelude::*;

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    fn make_meta_32mb() -> Ext4Meta {
        Ext4Meta::new(SIZE_BYTES, Some("TESTEXT4"))
    }

    #[test]
    fn test_ext4_superblock_magic() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT4 format failed");

        // Read superblock (offset 1024, 1024 bytes)
        let mut sb = [0u8; EXT4_SUPERBLOCK_SIZE];
        io.read_at(EXT4_SUPERBLOCK_OFFSET, &mut sb).unwrap();

        // Check magic number at offset 0x38
        let magic = u16::from_le_bytes(sb[0x38..0x3A].try_into().unwrap());
        assert_eq!(
            magic, EXT4_SUPERBLOCK_MAGIC,
            "EXT4 superblock magic mismatch"
        );

        // Check inode count
        let inode_count = u32::from_le_bytes(sb[0x00..0x04].try_into().unwrap());
        assert_eq!(inode_count, meta.inode_count, "Inode count mismatch");

        // Check block count
        let block_count = u32::from_le_bytes(sb[0x04..0x08].try_into().unwrap());
        assert_eq!(block_count, meta.block_count, "Block count mismatch");

        // Check first data block
        let first_data_block = u32::from_le_bytes(sb[0x14..0x18].try_into().unwrap());
        assert_eq!(
            first_data_block, meta.first_data_block,
            "First data block mismatch"
        );

        // Check log block size
        let log_block_size = u32::from_le_bytes(sb[0x18..0x1C].try_into().unwrap());
        let expected_log = meta.block_size.trailing_zeros() - 10;
        assert_eq!(log_block_size, expected_log, "Log block size mismatch");

        println!("✓ EXT4 superblock magic: 0x{magic:04X}");
        println!("✓ Inode count: {inode_count}");
        println!("✓ Block count: {block_count}");
        println!("✓ First data block: {first_data_block}");
    }

    #[test]
    fn test_ext4_bgdt_layout() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT4 format failed");

        // BGDT is at block 1 (after superblock at block 0)
        let bgdt_offset = (EXT4_SUPERBLOCK_BLOCK_NUMBER + 1) as u64 * meta.block_size as u64;
        let mut bgdt_buf = vec![0u8; EXT4_BGDT_ENTRY_SIZE * meta.group_count as usize];
        io.read_at(bgdt_offset, &mut bgdt_buf).unwrap();

        for group in 0..meta.group_count as usize {
            let offset = group * EXT4_BGDT_ENTRY_SIZE;
            let block_bitmap_lo =
                u32::from_le_bytes(bgdt_buf[offset..offset + 4].try_into().unwrap());
            let inode_bitmap_lo =
                u32::from_le_bytes(bgdt_buf[offset + 4..offset + 8].try_into().unwrap());
            let inode_table_lo =
                u32::from_le_bytes(bgdt_buf[offset + 8..offset + 12].try_into().unwrap());

            let layout = GroupLayout::compute(&meta, group as u32);

            assert_eq!(
                block_bitmap_lo, layout.block_bitmap_block,
                "Group {group}: block_bitmap mismatch"
            );
            assert_eq!(
                inode_bitmap_lo, layout.inode_bitmap_block,
                "Group {group}: inode_bitmap mismatch"
            );
            assert_eq!(
                inode_table_lo, layout.inode_table_block,
                "Group {group}: inode_table mismatch"
            );

            // Verify block_bitmap > group_start (after reserved blocks)
            assert!(
                block_bitmap_lo >= layout.group_start,
                "Group {group}: block_bitmap_lo ({block_bitmap_lo}) < group_start ({})",
                layout.group_start
            );

            println!(
                "✓ Group {group}: block_bitmap={block_bitmap_lo}, inode_bitmap={inode_bitmap_lo}, inode_table={inode_table_lo}"
            );
        }
    }

    #[test]
    fn test_ext4_block_bitmap() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT4 format failed");

        for group in 0..meta.group_count as usize {
            let layout = GroupLayout::compute(&meta, group as u32);
            let block_bitmap_block = layout.block_bitmap_block;

            // Read block bitmap
            let bitmap_offset = block_bitmap_block as u64 * meta.block_size as u64;
            let bitmap_size = (meta.blocks_per_group / 8) as usize;
            let mut bitmap = vec![0u8; bitmap_size];
            io.read_at(bitmap_offset, &mut bitmap).unwrap();

            // Helper to check bit
            let bit_is_set = |bit: usize| -> bool {
                let byte_index = bit / 8;
                let bit_mask = 1u8 << (bit % 8);
                byte_index < bitmap.len() && (bitmap[byte_index] & bit_mask) != 0
            };

            // Reserved blocks should be set
            for i in 0..layout.reserved_blocks as usize {
                assert!(
                    bit_is_set(i),
                    "Group {group}: reserved block {i} not set in bitmap"
                );
            }

            // Block bitmap block should be set
            let bb_bit = (block_bitmap_block - layout.group_start) as usize;
            assert!(
                bit_is_set(bb_bit),
                "Group {group}: block_bitmap bit {bb_bit} not set"
            );

            // Inode bitmap block should be set
            let ib_bit = (layout.inode_bitmap_block - layout.group_start) as usize;
            assert!(
                bit_is_set(ib_bit),
                "Group {group}: inode_bitmap bit {ib_bit} not set"
            );

            // Inode table blocks should be set
            for i in 0..layout.inode_table_blocks as usize {
                let bit = (layout.inode_table_block - layout.group_start + i as u32) as usize;
                assert!(
                    bit_is_set(bit),
                    "Group {group}: inode_table bit {bit} not set"
                );
            }

            // Group 0: root dir block should be set
            if group == 0 {
                let root_bit = (layout.first_data_block - layout.group_start) as usize;
                assert!(
                    bit_is_set(root_bit),
                    "Group 0: root_dir bit {root_bit} not set"
                );
            }

            println!("✓ Group {group}: block bitmap verified");
        }
    }

    #[test]
    fn test_ext4_inode_bitmap() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT4 format failed");

        // Only group 0 should have inode 2 (root) allocated
        let layout = GroupLayout::compute(&meta, 0);
        let inode_bitmap_block = layout.inode_bitmap_block;

        let bitmap_offset = inode_bitmap_block as u64 * meta.block_size as u64;
        let bitmap_size = (meta.inodes_per_group / 8) as usize;
        let mut bitmap = vec![0u8; bitmap_size];
        io.read_at(bitmap_offset, &mut bitmap).unwrap();

        // Helper to check bit
        let bit_is_set = |bit: usize| -> bool {
            let byte_index = bit / 8;
            let bit_mask = 1u8 << (bit % 8);
            byte_index < bitmap.len() && (bitmap[byte_index] & bit_mask) != 0
        };

        // Root inode (inode 2) should be set (bit 1, since 0-indexed)
        assert!(
            bit_is_set((EXT4_ROOT_INODE - 1) as usize),
            "Root inode bit not set in inode bitmap"
        );

        println!("✓ Group 0: inode bitmap verified (root inode allocated)");
    }

    #[test]
    fn test_ext4_root_directory() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT4 format failed");

        // Read root directory block
        let layout = GroupLayout::compute(&meta, 0);
        let root_block = layout.first_data_block;
        let root_offset = root_block as u64 * meta.block_size as u64;

        let mut root_buf = vec![0u8; meta.block_size as usize];
        io.read_at(root_offset, &mut root_buf).unwrap();

        // First entry should be "."
        let dot_inode = u32::from_le_bytes(root_buf[0..4].try_into().unwrap());
        let dot_rec_len = u16::from_le_bytes(root_buf[4..6].try_into().unwrap());
        let dot_name_len = root_buf[6];
        let dot_file_type = root_buf[7];
        let dot_name = &root_buf[8..8 + dot_name_len as usize];

        assert_eq!(
            dot_inode, EXT4_ROOT_INODE,
            ". entry should point to root inode"
        );
        assert_eq!(dot_name, b".", ". entry name mismatch");
        assert_eq!(
            dot_file_type, EXT4_FT_DIR,
            ". entry should be directory type"
        );

        // Second entry should be ".."
        let dotdot_offset = dot_rec_len as usize;
        let dotdot_inode = u32::from_le_bytes(
            root_buf[dotdot_offset..dotdot_offset + 4]
                .try_into()
                .unwrap(),
        );
        let dotdot_name_len = root_buf[dotdot_offset + 6];
        let dotdot_file_type = root_buf[dotdot_offset + 7];
        let dotdot_name =
            &root_buf[dotdot_offset + 8..dotdot_offset + 8 + dotdot_name_len as usize];

        assert_eq!(
            dotdot_inode, EXT4_ROOT_INODE,
            ".. entry should point to root inode (root parent is itself)"
        );
        assert_eq!(dotdot_name, b"..", ".. entry name mismatch");
        assert_eq!(
            dotdot_file_type, EXT4_FT_DIR,
            ".. entry should be directory type"
        );

        println!("✓ Root directory verified: '.' and '..' entries present");
    }

    #[test]
    fn test_ext4_format_complete() {
        let meta = make_meta_32mb();
        let mut buf = vec![0u8; SIZE_BYTES as usize];
        let mut io = MemRimIO::new(&mut buf);

        // Format should complete without errors
        Ext4Formatter::new(&mut io, &meta)
            .format(false)
            .expect("EXT4 format failed");

        // Verify superblock exists
        let mut sb = [0u8; 2];
        io.read_at(EXT4_SUPERBLOCK_OFFSET + 0x38, &mut sb).unwrap();
        let magic = u16::from_le_bytes(sb);
        assert_eq!(magic, EXT4_SUPERBLOCK_MAGIC);

        // Use checker for full validation
        let mut checker = Ext4Checker::new(&mut io, &meta);
        checker
            .fast_check()
            .expect("EXT4 fast_check failed after format");

        println!("✓ EXT4 format complete and validated");
    }
}
