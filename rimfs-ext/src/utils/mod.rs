// SPDX-License-Identifier: MIT
//! Shared EXT filesystem utilities (Block mapping, directory formatting, sparse super groups).

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

/// Read the authoritative primary group descriptor. Never substitute a generated layout.
pub(crate) fn read_group_descriptor<IO: rimio::RimRead + ?Sized>(
    io: &mut IO,
    meta: &crate::meta::ExtMeta,
    group: u32,
) -> rimio::RimIOResult<crate::types::ExtBlockGroupDesc> {
    if group as usize >= meta.group_count() || !matches!(meta.bgdt_entry_size, 32 | 64) {
        return Err(rimio::RimIOError::Invalid(
            "Invalid group descriptor geometry",
        ));
    }
    let offset = (meta.first_data_block as u64 + 1) * meta.block_size as u64
        + group as u64 * meta.bgdt_entry_size as u64;
    use rimio::RimReadStructExt;
    use zerocopy::{FromZeros, IntoBytes};
    if meta.bgdt_entry_size == 64 {
        return io.read_struct(offset);
    }
    // The legacy descriptor is the 32-byte prefix; absent extension fields are zero.
    let mut data = crate::types::ExtBlockGroupDesc::new_zeroed();
    io.read_at(offset, &mut data.as_mut_bytes()[..32])?;
    Ok(data)
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use rimio::MemRimIO;

    #[test]
    fn legacy_descriptor_read_does_not_consume_next_record() {
        let mut meta = crate::meta::ExtMeta::new(32 * 1024 * 1024, None).unwrap();
        meta.bgdt_entry_size = 32;
        let offset = (meta.first_data_block as usize + 1) * meta.block_size as usize;
        let mut bytes = vec![0u8; offset + 32];
        bytes[offset + 8..offset + 12].copy_from_slice(&0x12345678u32.to_le_bytes());
        let descriptor = read_group_descriptor(&mut MemRimIO::new(&mut bytes), &meta, 0).unwrap();
        assert_eq!({ descriptor.bg_inode_table_lo.get() }, 0x12345678);
        assert_eq!({ descriptor.bg_inode_table_hi.get() }, 0);
    }
}
