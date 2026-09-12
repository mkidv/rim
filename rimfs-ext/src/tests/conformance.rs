// SPDX-License-Identifier: MIT

//! Generic FsFilesystem contract conformance suite for EXT.

use crate::filesystem::Ext;
use crate::meta::ExtMeta;
use rimfs_core::testing::{FsCapabilities, test_fs_contract_compliance};
use rimio::MemRimIO;

#[test]
fn test_ext4_conformance() {
    let meta = ExtMeta::new(32 * 1024 * 1024, Some("EXT4_CONF")).expect("ExtMeta creation failed");
    let mut disk = vec![0u8; 32 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: None,
        boundary_size: Some(meta.block_size as usize),
        supports_symlinks: true,
        supports_nested_dirs: true,
        case_sensitive: true,
        max_file_size: u64::MAX,
    };

    test_fs_contract_compliance::<Ext, _, _>(&mut io, &meta, caps);
}
