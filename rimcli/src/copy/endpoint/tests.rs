// SPDX-License-Identifier: MIT

use super::*;
use std::path::Path;

#[test]
fn test_parse_host_paths() {
    let ep = CopyEndpoint::parse("host/dir/output").unwrap();
    assert_eq!(ep.host_path, Path::new("host/dir/output"));
    assert_eq!(ep.partition, None);
    assert_eq!(ep.internal_path, "/");
    assert!(!ep.is_explicit_image);

    let ep2 = CopyEndpoint::parse("/absolute/unix/path").unwrap();
    assert_eq!(ep2.host_path, Path::new("/absolute/unix/path"));
    assert_eq!(ep2.partition, None);
    assert_eq!(ep2.internal_path, "/");
    assert!(!ep2.is_explicit_image);
}

#[test]
fn test_parse_windows_host_paths() {
    let ep = CopyEndpoint::parse(r"C:\Users\test\output").unwrap();
    assert_eq!(ep.host_path, Path::new(r"C:\Users\test\output"));
    assert_eq!(ep.partition, None);
    assert_eq!(ep.internal_path, "/");
    assert!(!ep.is_explicit_image);

    let ep2 = CopyEndpoint::parse("D:/data/backup").unwrap();
    assert_eq!(ep2.host_path, Path::new("D:/data/backup"));
    assert_eq!(ep2.partition, None);
    assert_eq!(ep2.internal_path, "/");
    assert!(!ep2.is_explicit_image);

    let ep3 = CopyEndpoint::parse(r"\\?\C:\images\out").unwrap();
    assert_eq!(ep3.host_path, Path::new(r"\\?\C:\images\out"));
    assert_eq!(ep3.partition, None);
    assert_eq!(ep3.internal_path, "/");
    assert!(!ep3.is_explicit_image);
}

#[test]
fn test_parse_partitioned_endpoints() {
    let ep = CopyEndpoint::parse("disk.img:1:/EFI/BOOT").unwrap();
    assert_eq!(ep.host_path, Path::new("disk.img"));
    assert_eq!(ep.partition, Some(1));
    assert_eq!(ep.internal_path, "/EFI/BOOT");
    assert!(ep.is_explicit_image);

    let ep2 = CopyEndpoint::parse("disk.img:2:/etc/nginx/nginx.conf").unwrap();
    assert_eq!(ep2.host_path, Path::new("disk.img"));
    assert_eq!(ep2.partition, Some(2));
    assert_eq!(ep2.internal_path, "/etc/nginx/nginx.conf");
    assert!(ep2.is_explicit_image);

    let ep3 = CopyEndpoint::parse("disk.img:1").unwrap();
    assert_eq!(ep3.host_path, Path::new("disk.img"));
    assert_eq!(ep3.partition, Some(1));
    assert_eq!(ep3.internal_path, "/");
    assert!(ep3.is_explicit_image);
}

#[test]
fn test_parse_windows_partitioned_endpoints() {
    let ep = CopyEndpoint::parse(r"C:\images\disk.img:1:/EFI/BOOT").unwrap();
    assert_eq!(ep.host_path, Path::new(r"C:\images\disk.img"));
    assert_eq!(ep.partition, Some(1));
    assert_eq!(ep.internal_path, "/EFI/BOOT");
    assert!(ep.is_explicit_image);

    let ep2 = CopyEndpoint::parse("C:/images/disk.img:2:/etc").unwrap();
    assert_eq!(ep2.host_path, Path::new("C:/images/disk.img"));
    assert_eq!(ep2.partition, Some(2));
    assert_eq!(ep2.internal_path, "/etc");
    assert!(ep2.is_explicit_image);

    let ep3 = CopyEndpoint::parse(r"\\?\C:\images\disk.img:1:/").unwrap();
    assert_eq!(ep3.host_path, Path::new(r"\\?\C:\images\disk.img"));
    assert_eq!(ep3.partition, Some(1));
    assert_eq!(ep3.internal_path, "/");
    assert!(ep3.is_explicit_image);
}

#[test]
fn test_parse_unpartitioned_endpoints() {
    let ep = CopyEndpoint::parse("rootfs.ext4:/etc").unwrap();
    assert_eq!(ep.host_path, Path::new("rootfs.ext4"));
    assert_eq!(ep.partition, None);
    assert_eq!(ep.internal_path, "/etc");
    assert!(ep.is_explicit_image);

    let ep2 = CopyEndpoint::parse("archive.tar:/var/log").unwrap();
    assert_eq!(ep2.host_path, Path::new("archive.tar"));
    assert_eq!(ep2.partition, None);
    assert_eq!(ep2.internal_path, "/var/log");
    assert!(ep2.is_explicit_image);

    let ep3 = CopyEndpoint::parse(r"C:\data\rootfs.ext4:/etc").unwrap();
    assert_eq!(ep3.host_path, Path::new(r"C:\data\rootfs.ext4"));
    assert_eq!(ep3.partition, None);
    assert_eq!(ep3.internal_path, "/etc");
    assert!(ep3.is_explicit_image);
}

#[test]
fn test_parse_invalid_endpoints() {
    assert!(CopyEndpoint::parse("").is_err());
    assert!(CopyEndpoint::parse("   ").is_err());

    // Partition 0 is invalid
    let err0 = CopyEndpoint::parse("disk.img:0:/etc")
        .unwrap_err()
        .to_string();
    assert!(err0.contains("Invalid partition number 0"));

    let err0_short = CopyEndpoint::parse("disk.img:0").unwrap_err().to_string();
    assert!(err0_short.contains("Invalid partition number 0"));

    // Malformed partition selector
    let err_bad = CopyEndpoint::parse("disk.img:p1:/etc")
        .unwrap_err()
        .to_string();
    assert!(err_bad.contains("Invalid partition selector 'p1'"));

    // Empty image before colon
    let err_empty = CopyEndpoint::parse(":1:/etc").unwrap_err().to_string();
    assert!(err_empty.contains("cannot be empty"));
}
