// SPDX-License-Identifier: MIT

//! Generic FsFilesystem contract conformance suite for TAR.

use crate::filesystem::Tar;
use crate::meta::TarMeta;
use alloc::vec;
use rimfs_core::testing::{FsCapabilities, test_fs_contract_compliance};
use rimio::MemRimIO;

#[test]
fn test_tar_conformance() {
    let meta = TarMeta::default();
    let mut disk = vec![0u8; 128 * 1024];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: Some(&[]),
        boundary_size: Some(crate::types::TAR_BLOCK_SIZE),
        supports_symlinks: true,
        supports_nested_dirs: true,
        case_sensitive: true,
        max_file_size: u64::MAX,
    };

    test_fs_contract_compliance::<Tar, _, _>(&mut io, &meta, caps);
}
