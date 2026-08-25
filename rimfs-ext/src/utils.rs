// SPDX-License-Identifier: MIT

use crate::{group_layout::GroupLayout, meta::ExtMeta};

pub mod block_map;

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

pub fn compute_used_blocks_in_group(group: GroupLayout, _params: &ExtMeta) -> u32 {
    let mut used = group.metadata_blocks();

    if group.group_id == 0 {
        used += 2; // root dir block + lost+found block
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
