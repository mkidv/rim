// SPDX-License-Identifier: MIT

//! Block Map (Legacy) addressing logic for Ext2/3.

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The 15-entry array stored in i_block for Ext2/3
/// 12 Direct + 1 Indirect + 1 Double + 1 Triple
#[derive(Debug, Clone, Copy, IntoBytes, FromBytes, KnownLayout, Immutable)]
#[repr(C)]
#[derive(Default)]
pub struct BlockMapArray {
    pub direct: [u32; 12],
    pub indirect: u32,
    pub double_indirect: u32,
    pub triple_indirect: u32,
}
