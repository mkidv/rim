// SPDX-License-Identifier: MIT
//! Bare-Metal RIM.efi application layout definition for RIM.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use rimfs::core::resolver::FsTreeResolver;
use rimfs::tar::{TarMeta, TarResolver};
use rimgen::guid::{GuidGenerator, SeededGuidGenerator};
use rimgen::layout::{Filesystem, Layout, Partition, PartitionKind};
use rimio::SliceRimIO;

pub const UEFI_ESP_SIZE_SECTORS: u64 = 143_360; // 70 MiB (143,360 sectors @ 512B)
// Total 72 MiB disk image: (143,360 + 2 * 2048) * 512 = 75,497,472 bytes
pub const UEFI_TOTAL_SIZE_BYTES: usize = (143_360 + 2 * 2048) * 512;
pub const UEFI_SEED: u64 = 0x5545_4649_5249_4D31; // "UEFIRIM1"

/// Builds the bootable UEFI Application `Layout` from raw payload TAR bytes.
pub fn make_uefi_layout<'a>(
    tar_bytes: &'a [u8],
) -> Result<(Layout<'a>, SeededGuidGenerator), String> {
    let mut guid_gen = SeededGuidGenerator::new(UEFI_SEED);
    let mut layout = Layout::new(guid_gen.generate_guid());

    let meta = TarMeta::default();
    let mut tar_io = SliceRimIO::new(tar_bytes);
    let mut resolver = TarResolver::new(&mut tar_io, &meta);

    // 1. Build EFI System Partition (FAT32)
    let esp_root = resolver
        .resolve_tree("esp/*")
        .map_err(|e| format!("Failed to parse ESP partition from TAR payload: {e}"))?;
    let esp_part = Partition::new(
        "ESP",
        PartitionKind::Esp,
        Filesystem::Fat32,
        UEFI_ESP_SIZE_SECTORS,
        guid_gen.generate_guid(),
    )
    .with_bootable(true)
    .with_label("RIM_EFI")
    .with_uuid("1234-5678")
    .with_root(esp_root);

    layout = layout.add_partition(esp_part);
    Ok((layout, guid_gen))
}
