// SPDX-License-Identifier: MIT

//! Generic FsFilesystem contract conformance suite for ZIP.

use crate::filesystem::Zip;
use crate::meta::ZipMeta;
use alloc::vec;
use rimfs_core::testing::{FsCapabilities, test_fs_contract_compliance};
use rimio::MemRimIO;

#[test]
fn test_zip_conformance() {
    let meta = ZipMeta::default();
    let mut disk = vec![0u8; 128 * 1024];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: Some(&[]),
        boundary_size: None,
        supports_symlinks: false,
        supports_nested_dirs: true,
        case_sensitive: true,
        max_file_size: u64::MAX,
    };

    test_fs_contract_compliance::<Zip, _, _>(&mut io, &meta, caps);
}
