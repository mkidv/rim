// SPDX-License-Identifier: MIT

//! Generic FsFilesystem contract conformance suite for ISO 9660.

use crate::filesystem::Iso;
use crate::meta::IsoMeta;
use alloc::vec;
use rimfs_core::testing::{FsCapabilities, test_fs_contract_compliance};
use rimio::MemRimIO;

#[test]
fn test_iso_conformance() {
    let meta = IsoMeta::default();
    let mut disk = vec![0u8; 200 * crate::types::ISO_SECTOR_SIZE];
    let mut io = MemRimIO::new(&mut disk);

    let caps = FsCapabilities {
        formatted_root_entries: Some(&[]),
        boundary_size: Some(crate::types::ISO_SECTOR_SIZE),
        supports_symlinks: false,
        supports_nested_dirs: true,
        case_sensitive: true,
        max_file_size: u64::MAX,
    };

    test_fs_contract_compliance::<Iso, _, _>(&mut io, &meta, caps);
}
