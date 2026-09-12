// SPDX-License-Identifier: MIT

//! Generic FsFilesystem contract conformance suite for exFAT.

use crate::filesystem::ExFat;
use crate::meta::ExFatMeta;
use rimfs_core::testing::{FsCapabilities, test_fs_contract_compliance};
use rimio::MemRimIO;

#[test]
fn test_exfat_conformance() {
    let meta =
        ExFatMeta::new(32 * 1024 * 1024, Some("EXFAT_CONF")).expect("ExFatMeta creation failed");
    let mut disk = vec![0u8; 32 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: None,
        boundary_size: Some(meta.bytes_per_cluster as usize),
        supports_symlinks: false,
        supports_nested_dirs: true,
        case_sensitive: false,
        max_file_size: u64::MAX,
    };

    test_fs_contract_compliance::<ExFat, _, _>(&mut io, &meta, caps);
}
