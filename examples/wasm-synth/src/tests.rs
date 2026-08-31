// SPDX-License-Identifier: MIT
//! Native automated tests for the WASM demo layout and synthesizer.

use super::*;
use rimfs::core::resolver::FsTreeResolver;
use rimfs_ext::{ExtMeta, ExtResolver};
use rimfs_fat::{FatMeta, FatResolver};
use rimio::RimIO;
use rimpart::gpt::decode_gpt_name;

#[test]
fn test_wasm_demo_layout_synthesis_and_validation() {
    let (mut image, report) = synthesize_demo_image().expect("synthesize_demo_image failed");
    assert_eq!(image.len(), DEMO_IMAGE_SIZE_BYTES);
    assert!(report.contains("\"status\":\"ok\""));

    let mut io = MemRimIO::new(&mut image);

    // 1. Validate GPT
    let (_hdr, entries) =
        rimpart::gpt::read_gpt_with_sector(&mut io, 512).expect("GPT read failed");
    assert_eq!(entries.len(), 2);
    assert_eq!(decode_gpt_name(&entries[0].name), "ESP");
    assert_eq!(decode_gpt_name(&entries[1].name), "rootfs");

    // 2. Validate FAT32 partition
    let p1_offset = entries[0].start_lba * 512;
    let p1_len = (entries[0].end_lba - entries[0].start_lba + 1) * 512;
    io.set_offset(p1_offset);

    let fat_meta = FatMeta::new_fat32(p1_len, Some("BOOT")).expect("FAT32 meta failed");
    let mut fat_resolver = FatResolver::new(&mut io, &fat_meta);
    let fat_tree = fat_resolver
        .resolve_tree("/*")
        .expect("FAT32 resolve_tree failed");
    assert!(
        format!("{fat_tree}")
            .to_ascii_lowercase()
            .contains("readme.txt")
    );

    // 3. Validate EXT4 partition
    let p2_offset = entries[1].start_lba * 512;
    let p2_len = (entries[1].end_lba - entries[1].start_lba + 1) * 512;
    io.set_offset(p2_offset);

    let ext_meta = ExtMeta::new(p2_len, Some("ROOTFS")).unwrap();
    let mut ext_resolver = ExtResolver::new(&mut io, &ext_meta);
    let ext_tree = ext_resolver
        .resolve_tree("/*")
        .expect("EXT4 resolve_tree failed");
    let ext_tree_str = format!("{ext_tree}");
    assert!(ext_tree_str.contains("hello.txt"));
    assert!(ext_tree_str.contains("etc"));
    assert!(ext_tree_str.contains("rim.conf"));
    assert!(ext_tree_str.contains("hello_link"));
}

#[test]
fn test_wasm_demo_deterministic_reproducibility() {
    let (img1, _) = synthesize_demo_image().expect("first synthesis failed");
    let (img2, _) = synthesize_demo_image().expect("second synthesis failed");

    assert_eq!(img1.len(), img2.len());
    assert_eq!(
        img1, img2,
        "WASM demo builds must be 100% byte-for-byte deterministic"
    );
}

#[test]
fn test_inspect_demo_image_reports_partitions() {
    let (image, _) = synthesize_demo_image().expect("synthesize_demo_image failed");
    let report = inspect_disk_image(&image).expect("inspect_disk_image failed");

    assert!(report.contains("\"kind\":\"inspect\""));
    assert!(report.contains("\"gpt_present\":true"));
    assert!(report.contains("\"partitions_count\":2"));
    assert!(report.contains("\"name\":\"ESP\""));
    assert!(report.contains("\"name\":\"rootfs\""));
}
