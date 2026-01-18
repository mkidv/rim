// SPDX-License-Identifier: MIT

use crate::fs::ext4::{constant::*, group_layout::GroupLayout, meta::Ext4Meta};

pub fn is_sparse_super_group(group_id: u32) -> bool {
    if group_id == 0 {
        return true;
    }

    for &base in &[3, 5, 7] {
        let mut p = 1;
        while p <= group_id {
            if p == group_id {
                return true;
            }
            p *= base;
        }
    }

    false
}

pub fn compute_used_blocks_in_group(group: GroupLayout, params: &Ext4Meta) -> u32 {
    let table_blocks =
        (params.inodes_per_group * EXT4_DEFAULT_INODE_SIZE / params.block_size).div_ceil(1);

    let mut used = group.reserved_blocks;
    used += 1; // block_bitmap
    used += 1; // inode_bitmap
    used += table_blocks;

    if group.group_id == 0 {
        used += 1; // root dir block
    }

    used
}

pub fn compute_used_inodes_in_group(group_id: u32) -> u32 {
    if group_id == 0 {
        2 // bad block inode + root dir inode
    } else {
        0
    }
}
