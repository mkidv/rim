// SPDX-License-Identifier: MIT
//! Shared EXT filesystem utilities (Block mapping, directory formatting, sparse super groups).

pub mod block_map;
pub mod dir;

pub use dir::*;

/// Returns true if the group contains a backup superblock and BGDT
/// under the `sparse_super` feature (group 0, 1, 3, 5, 7, and powers thereof).
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
