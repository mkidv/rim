// SPDX-License-Identifier: MIT

//! Generic FsFilesystem contract conformance suite for FAT.

use crate::filesystem::Fat;
use crate::meta::FatMeta;
use rimfs_core::testing::{FsCapabilities, test_fs_contract_compliance};
use rimio::MemRimIO;

#[test]
fn test_fat32_conformance() {
    let meta =
        FatMeta::new_fat32(32 * 1024 * 1024, Some("FAT32_CONF")).expect("FatMeta FAT32 failed");
    let mut disk = vec![0u8; 32 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: None,
        boundary_size: Some(meta.bytes_per_cluster as usize),
        supports_symlinks: false,
        supports_nested_dirs: true,
        case_sensitive: false,
        max_file_size: 4 * 1024 * 1024 * 1024 - 1,
    };

    test_fs_contract_compliance::<Fat, _, _>(&mut io, &meta, caps);
}

#[test]
fn test_fat16_conformance() {
    let meta =
        FatMeta::new_fat16(16 * 1024 * 1024, Some("FAT16_CONF")).expect("FatMeta FAT16 failed");
    let mut disk = vec![0u8; 16 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: None,
        boundary_size: Some(meta.bytes_per_cluster as usize),
        supports_symlinks: false,
        supports_nested_dirs: true,
        case_sensitive: false,
        max_file_size: 2 * 1024 * 1024 * 1024 - 1,
    };

    test_fs_contract_compliance::<Fat, _, _>(&mut io, &meta, caps);
}
