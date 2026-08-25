// SPDX-License-Identifier: MIT

use rimgen::{DiskLayout, Filesystem, ImageBuilder, Partition, Size};
use rimimg::ImageFormat;
use rimio::prelude::*;
use std::fs::File;
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
    let layout = DiskLayout {
        base_dir: PathBuf::from("."),
        partitions: vec![
            Partition {
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
            Partition {
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
            Partition {
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
    let mut builder = ImageBuilder::new(layout);
    builder.build_to_file(&img_path).unwrap();
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

    // 4. Convert to all container formats
    rimimg::convert(&img_path, &vhd_path).unwrap();
    rimimg::convert(&img_path, &qcow2_path).unwrap();
    rimimg::convert(&img_path, &vdi_path).unwrap();
    rimimg::convert(&img_path, &vmdk_path).unwrap();

    // 5. Inspect and detect container formats
    let mut f = File::open(&vhd_path).unwrap();
    assert_eq!(ImageFormat::from_file(&mut f).unwrap(), ImageFormat::Vhd);

    let mut f = File::open(&qcow2_path).unwrap();
    assert_eq!(ImageFormat::from_file(&mut f).unwrap(), ImageFormat::Qcow2);

    let mut f = File::open(&vdi_path).unwrap();
    assert_eq!(ImageFormat::from_file(&mut f).unwrap(), ImageFormat::Vdi);

    let mut f = File::open(&vmdk_path).unwrap();
    assert_eq!(ImageFormat::from_file(&mut f).unwrap(), ImageFormat::Vmdk);
}
