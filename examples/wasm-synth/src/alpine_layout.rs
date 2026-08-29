// SPDX-License-Identifier: MIT
//! Bootable Alpine Linux layout definition for RIM.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use rimfs::core::resolver::FsTreeResolver;
use rimfs::tar::{TarMeta, TarResolver};
use rimgen::guid::{GuidGenerator, SeededGuidGenerator};
use rimgen::layout::{Filesystem, Layout, Partition, PartitionKind};
use rimio::SliceRimIO;

pub const ALPINE_ESP_SIZE_SECTORS: u64 = 131_072; // 64 MiB (131,072 sectors @ 512B)
pub const ALPINE_ROOTFS_SIZE_SECTORS: u64 = 327_680; // 160 MiB (327,680 sectors @ 512B)
// Exact total bytes including 3x 1 MiB alignment sectors (464,896 sectors * 512 = 238,026,752 bytes)
pub const ALPINE_TOTAL_SIZE_BYTES: usize = (131_072 + 327_680 + 3 * 2048) * 512;
pub const ALPINE_SEED: u64 = 0x414C_5049_4E45_5249; // "ALPINERI"

/// Builds the bootable Alpine Linux `Layout` from raw uncompressed payload TAR bytes.
pub fn make_alpine_layout<'a>(
    tar_bytes: &'a [u8],
) -> Result<(Layout<'a>, SeededGuidGenerator), String> {
    let mut guid_gen = SeededGuidGenerator::new(ALPINE_SEED);
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
        ALPINE_ESP_SIZE_SECTORS,
        guid_gen.generate_guid(),
    )
    .with_bootable(true)
    .with_label("BOOT")
    .with_uuid("ABCD-1234")
    .with_root(esp_root);

    // 2. Build Linux root filesystem (EXT4)
    let rootfs_root = resolver
        .resolve_tree("rootfs/*")
        .map_err(|e| format!("Failed to parse rootfs partition from TAR payload: {e}"))?;
    let rootfs_part = Partition::new(
        "rootfs",
        PartitionKind::Linux,
        Filesystem::Ext4,
        ALPINE_ROOTFS_SIZE_SECTORS,
        guid_gen.generate_guid(),
    )
    .with_label("ROOTFS")
    .with_uuid("550e8400-e29b-41d4-a716-446655440000")
    .with_root(rootfs_root);

    layout = layout.add_partition(esp_part).add_partition(rootfs_part);
    Ok((layout, guid_gen))
}
