// SPDX-License-Identifier: MIT
//! Pure in-memory disk synthesis test using portable RimIO and ResolvedLayout.

use rimfs::core::resolver::{FileAttributes, FsNode};
use rimgen::builder::build_on_io;
use rimgen::guid::{GuidGenerator, SeededGuidGenerator};
use rimgen::layout::{Filesystem, Layout, Partition, PartitionKind};
use rimio::MemRimIO;
use rimpart::gpt::decode_gpt_name;

#[test]
fn test_portable_in_memory_multi_fs() {
    let total_size = 128 * 1024 * 1024; // 128 MiB
    let mut buf = vec![0u8; total_size];
    let mut io = MemRimIO::new(&mut buf);
    let mut guid_gen = SeededGuidGenerator::new(0xDEAD_BEEF);

    let mut layout = Layout::new(guid_gen.generate_guid());

    // 1. ESP FAT32 partition
    let boot_file = FsNode::new_file("BOOTX64.EFI", b"UEFI BINARY PAYLOAD".to_vec());
    let boot_dir = FsNode::Dir {
        name: "BOOT".into(),
        children: vec![boot_file],
        attr: FileAttributes::new_dir(),
    };
    let efi_dir = FsNode::Dir {
        name: "EFI".into(),
        children: vec![boot_dir],
        attr: FileAttributes::new_dir(),
    };
    let esp_node = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: vec![efi_dir],
    };

    let esp_part = Partition::new(
        "ESP",
        PartitionKind::Esp,
        Filesystem::Fat32,
        65536, // 32 MiB
        guid_gen.generate_guid(),
    )
    .with_bootable(true)
    .with_label("EFI")
    .with_root(esp_node);

    // 2. Linux EXT4 partition
    let host_file = FsNode::new_file("hostname", b"rim-box\n".to_vec());
    let etc_dir = FsNode::Dir {
        name: "etc".into(),
        children: vec![host_file],
        attr: FileAttributes::new_dir(),
    };
    let root_node = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: vec![etc_dir],
    };

    let linux_part = Partition::new(
        "rootfs",
        PartitionKind::Linux,
        Filesystem::Ext4,
        40960, // 20 MiB
        guid_gen.generate_guid(),
    )
    .with_label("ROOT")
    .with_root(root_node);

    // 3. NTFS Data partition
    let readme_file = FsNode::new_file("readme.txt", b"NTFS partition content".to_vec());
    let data_node = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: vec![readme_file],
    };

    let ntfs_part = Partition::new(
        "data",
        PartitionKind::Data,
        Filesystem::Ntfs,
        20480, // 10 MiB
        guid_gen.generate_guid(),
    )
    .with_label("DATA")
    .with_root(data_node);

    layout = layout
        .add_partition(esp_part)
        .add_partition(linux_part)
        .add_partition(ntfs_part);

    let report = build_on_io(&mut layout, &mut io, |_| {}).expect("build_on_io failed");

    assert_eq!(report.partitions.len(), 3);
    assert_eq!(report.partitions[0].name, "ESP");
    assert_eq!(report.partitions[1].name, "rootfs");
    assert_eq!(report.partitions[2].name, "data");

    // Validate GPT table on io
    let (_hdr, entries) =
        rimpart::gpt::read_gpt_with_sector(&mut io, 512).expect("failed to read GPT");
    assert_eq!(entries.len(), 3);
    assert_eq!(decode_gpt_name(&entries[0].name), "ESP");
    assert_eq!(decode_gpt_name(&entries[1].name), "rootfs");
    assert_eq!(decode_gpt_name(&entries[2].name), "data");
}
