// SPDX-License-Identifier: MIT

//! Low-level EXT4 forensic analysis and superblock / BGDT decoding.

use rimfs::ext::constant::*;
use rimfs::ext::types::{ExtBlockGroupDesc, ExtSuperblock};
use rimio::RimIO;
use zerocopy::FromBytes;

pub fn analyze_ext(io: &mut dyn RimIO, partition_offset: u64) {
    let sb_offset = partition_offset + EXT_SUPERBLOCK_OFFSET;
    let mut sb_buf = [0u8; 1024];
    if io.read_at(sb_offset, &mut sb_buf).is_err() {
        println!(
            "[-] Failed to read Ext superblock at offset 0x{:X}",
            sb_offset
        );
        return;
    }

    if let Ok((sb, _)) = ExtSuperblock::ref_from_prefix(&sb_buf) {
        let magic = sb.s_magic.get();
        if magic != EXT_SUPERBLOCK_MAGIC {
            println!(
                "[-] Invalid Ext superblock magic: 0x{:04X} (expected 0xEF53)",
                magic
            );
            return;
        }

        let block_size = 1024u64 << sb.s_log_block_size.get();
        let inodes_count = sb.s_inodes_count.get();
        let blocks_count = sb.s_blocks_count_lo.get();
        let free_blocks = sb.s_free_blocks_count_lo.get();
        let free_inodes = sb.s_free_inodes_count.get();
        let blocks_per_group = sb.s_blocks_per_group.get();
        let inodes_per_group = sb.s_inodes_per_group.get();
        let vol_name = String::from_utf8_lossy(&sb.s_volume_name);

        println!("\n=== EXT4 Filesystem Forensic Overview ===");
        println!("  Superblock Offset:  0x{:X}", sb_offset);
        println!("  Volume Name:        {}", vol_name.trim_matches('\0'));
        println!("  Magic:              0x{:04X} (Valid EXT4)", magic);
        println!("  Block Size:         {} bytes", block_size);
        println!("  Total Inodes:       {}", inodes_count);
        println!("  Total Blocks:       {}", blocks_count);
        println!("  Free Inodes:        {}", free_inodes);
        println!("  Free Blocks:        {}", free_blocks);
        println!("  Blocks Per Group:   {}", blocks_per_group);
        println!("  Inodes Per Group:   {}", inodes_per_group);
        println!("  First Inode:        {}", sb.s_first_ino.get());
        println!("  Inode Size:         {} bytes", sb.s_inode_size.get());
        println!("  Volume UUID:        {:02X?}", sb.s_uuid);
        println!("  Compat Features:    0x{:08X}", sb.s_feature_compat.get());
        println!(
            "  Incompat Features:  0x{:08X}",
            sb.s_feature_incompat.get()
        );
        println!(
            "  RO Compat Features: 0x{:08X}",
            sb.s_feature_ro_compat.get()
        );

        // Group count
        if blocks_per_group > 0 {
            let num_groups = blocks_count.div_ceil(blocks_per_group);
            println!("  Block Groups Count: {}", num_groups);

            // Read group descriptor 0
            let bgdt_offset = partition_offset
                + (if block_size == 1024 {
                    2 * 1024
                } else {
                    block_size
                });
            let mut bgd_buf = [0u8; 64];
            if io.read_at(bgdt_offset, &mut bgd_buf).is_ok()
                && let Ok((bgd, _)) = ExtBlockGroupDesc::ref_from_prefix(&bgd_buf)
            {
                println!("\n  --- Block Group 0 Descriptor ---");
                println!(
                    "    Block Bitmap:     Block {}",
                    bgd.bg_block_bitmap_lo.get()
                );
                println!(
                    "    Inode Bitmap:     Block {}",
                    bgd.bg_inode_bitmap_lo.get()
                );
                println!(
                    "    Inode Table:      Block {}",
                    bgd.bg_inode_table_lo.get()
                );
                println!(
                    "    Free Blocks:      {}",
                    bgd.bg_free_blocks_count_lo.get()
                );
                println!(
                    "    Free Inodes:      {}",
                    bgd.bg_free_inodes_count_lo.get()
                );
                println!("    Used Dirs Count:  {}", bgd.bg_used_dirs_count_lo.get());
            }
        }
    }
}
