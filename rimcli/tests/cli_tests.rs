// SPDX-License-Identifier: MIT

use rimgen::{Filesystem, ImageBuilder, LayoutConfig, PartitionConfig, Size};
use rimimg::ImageFormat;
use std::fs::File;
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

    let mut builder = ImageBuilder::new(layout);
    builder.build_to_file(&img_path).unwrap();

    // 2. Test check on RAW
    // Call the check logic
    rimimg::convert(&img_path, &vhd_path).unwrap();

    let mut f = File::open(&vhd_path).unwrap();
    assert_eq!(ImageFormat::from_file(&mut f).unwrap(), ImageFormat::Vhd);
}
