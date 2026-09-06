// SPDX-License-Identifier: MIT

use rimgen::{Filesystem, LayoutConfig, PartitionConfig, Size};
use rimimg::ImageFormat;
use rimio::prelude::*;
use std::fs::File;
use std::fs::OpenOptions;
use std::path::Path;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_declarative_builder_and_format_conversions() {
    let dir = tempdir().unwrap();
    let img_path = dir.path().join("test_disk.img");
    let vhd_path = dir.path().join("test_disk.vhd");
    let qcow2_path = dir.path().join("test_disk.qcow2");
    let vdi_path = dir.path().join("test_disk.vdi");
    let vmdk_path = dir.path().join("test_disk.vmdk");

    // 1. Build a multi-partition layout
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
            PartitionConfig {
                name: "DATA_EXFAT".to_string(),
                size: Size::Fixed(16),
                fs: Filesystem::ExFat,
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

    // 2. Build RAW image
    build_config_to_file(&layout, &img_path, ImageFormat::Raw);
    assert!(img_path.exists());

    // 3. Scan RAW image partitions via rimpart
    {
        let mut file = File::open(&img_path).unwrap();
        let mut io = StdRimIO::new(&mut file);
        let scan = rimpart::scan_disk_with_sector(&mut io, 512).unwrap();
        assert_eq!(scan.partitions.len(), 3);
        assert_eq!(scan.partitions[0].name, "ESP");
        assert_eq!(scan.partitions[1].name, "DATA_EXT4");
        assert_eq!(scan.partitions[2].name, "DATA_EXFAT");
    }

    // 4. Build directly to all container formats
    for (path, format) in [
        (&vhd_path, ImageFormat::Vhd),
        (&qcow2_path, ImageFormat::Qcow2),
        (&vdi_path, ImageFormat::Vdi),
        (&vmdk_path, ImageFormat::Vmdk),
    ] {
        build_config_to_file(&layout, path, format);
        assert_eq!(detect_format(path), format);
        assert_partition_layout(path);
    }
}

#[test]
fn test_build_without_partition_table() {
    let layout = LayoutConfig {
        base_dir: PathBuf::from("."),
        partitions: vec![PartitionConfig {
            name: "DATA".to_string(),
            size: Size::Fixed(16),
            fs: Filesystem::Fat16,
            mountpoint: None,
            index: None,
            bootable: false,
            kind: None,
            guid: None,
            payload: None,
            label: None,
            uuid: None,
        }],
        disk: None,
    };
    let options = rimgen::BuildOptions {
        partition_table: rimgen::PartitionTable::None,
    };
    let raw_len = rimgen::calculate_total_disk_sectors_from_config_with_options(&layout, options)
        .unwrap()
        * 512;
    let mut buffer = vec![0; raw_len as usize];
    let mut io = MemRimIO::new(&mut buffer);

    let report = rimgen::build_config_on_io_with_options(&layout, &mut io, options).unwrap();

    assert_eq!(report.partitions.len(), 1);
    assert_eq!(report.partitions[0].start_lba, 0);
    assert!(rimpart::gpt::read_gpt_with_sector(&mut io, 512).is_err());
}

fn build_config_to_file(layout: &LayoutConfig, output: &Path, format: ImageFormat) {
    let raw_len = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(layout) * 512;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)
        .unwrap();
    let mut file_io = FileRimIO::new(file);

    if format == ImageFormat::Raw {
        file_io.set_len(raw_len).unwrap();
        rimgen::build_config_on_io(layout, &mut file_io).unwrap();
    } else {
        let mut image = rimimg::create_image_io(
            &mut file_io,
            raw_len,
            format,
            rimimg::ImageOptions::deterministic(1),
        )
        .unwrap();
        rimgen::build_config_on_io(layout, &mut image).unwrap();
        image.finish().unwrap();
    }
}

fn detect_format(path: &Path) -> ImageFormat {
    let file = File::open(path).unwrap();
    let mut io = FileRimIO::new(file);
    ImageFormat::from_io(&mut io).unwrap()
}

fn assert_partition_layout(path: &Path) {
    let file = File::open(path).unwrap();
    let mut file_io = FileRimIO::new(file);
    let mut disk = rimimg::open_image_io(&mut file_io).unwrap();
    let scan = rimpart::scan_disk_with_sector(&mut disk, 512).unwrap();

    assert_eq!(scan.partitions.len(), 3);
    assert_eq!(scan.partitions[0].name, "ESP");
    assert_eq!(scan.partitions[1].name, "DATA_EXT4");
    assert_eq!(scan.partitions[2].name, "DATA_EXFAT");
}
