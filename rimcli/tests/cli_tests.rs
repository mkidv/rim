// SPDX-License-Identifier: MIT

use rimgen::{Filesystem, LayoutConfig, PartitionConfig, Size};
use rimimg::ImageFormat;
use rimio::prelude::*;
use std::fs::File;
use std::fs::OpenOptions;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_cli_builder_and_check() {
    let dir = tempdir().unwrap();
    let img_path = dir.path().join("cli_test_disk.img");
    let vhd_path = dir.path().join("cli_test_disk.vhd");

    // 1. Build layout
    let layout = LayoutConfig {
        base_dir: PathBuf::from("."),
        partitions: vec![
            PartitionConfig {
                name: "ESP".to_string(),
                size: Size::Fixed(16),
                fs: Filesystem::Fat16,
                mountpoint: None,
                index: None,
                bootable: true,
                kind: None,
                guid: None,
                payload: None,
                label: None,
                uuid: None,
            },
            PartitionConfig {
                name: "DATA_EXT4".to_string(),
                size: Size::Fixed(16),
                fs: Filesystem::Ext4,
                mountpoint: None,
                index: None,
                bootable: false,
                kind: None,
                guid: None,
                payload: None,
                label: None,
                uuid: None,
            },
        ],
        disk: None,
    };

    let raw_len = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(&layout) * 512;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&img_path)
        .unwrap();
    let mut io = FileRimIO::new(file);
    io.set_len(raw_len).unwrap();
    rimgen::build_config_on_io(&layout, &mut io).unwrap();

    // 2. Test check on RAW
    // Call the check logic
    let input = File::open(&img_path).unwrap();
    let input_len = input.metadata().unwrap().len();
    let output = File::create(&vhd_path).unwrap();
    let mut src = FileRimIO::new(input);
    let mut dst = FileRimIO::new(output);
    rimimg::vhd::wrap_raw_as_vhd_io(
        &mut src,
        &mut dst,
        input_len,
        rimimg::ImageOptions::deterministic(1),
    )
    .unwrap();

    let f = File::open(&vhd_path).unwrap();
    let mut io = FileRimIO::new(f);
    assert_eq!(ImageFormat::from_io(&mut io).unwrap(), ImageFormat::Vhd);
}
