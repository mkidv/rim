// SPDX-License-Identifier: MIT
//! Validates that building the same layout with deterministic GUID generation
//! produces 100% byte-for-byte identical images.

use rimfs::core::resolver::{FileAttributes, FsNode};
use rimfs::core::time::OffsetDateTime;
use rimgen::builder::build_on_io;
use rimgen::guid::{GuidGenerator, SeededGuidGenerator};
use rimgen::layout::{Filesystem, Layout, Partition, PartitionKind};
use rimio::MemRimIO;

fn make_layout(seed: u64) -> (Layout<'static>, SeededGuidGenerator) {
    let mut guid_gen = SeededGuidGenerator::new(seed);
    let mut layout = Layout::new(guid_gen.generate_guid());

    let mut file_attr = FileAttributes::new_file();
    file_attr.created = Some(OffsetDateTime::UNIX_EPOCH);
    file_attr.modified = Some(OffsetDateTime::UNIX_EPOCH);
    file_attr.accessed = Some(OffsetDateTime::UNIX_EPOCH);

    let mut dir_attr = FileAttributes::new_dir();
    dir_attr.created = Some(OffsetDateTime::UNIX_EPOCH);
    dir_attr.modified = Some(OffsetDateTime::UNIX_EPOCH);
    dir_attr.accessed = Some(OffsetDateTime::UNIX_EPOCH);

    let boot_file = FsNode::File {
        name: "boot.cfg".into(),
        source: Box::new(rimio::VecRimIO::new(b"timeout=5\ndefault=0\n".to_vec())),
        attr: file_attr.clone(),
    };
    let efi_dir = FsNode::Dir {
        name: "EFI".into(),
        children: vec![boot_file],
        attr: dir_attr.clone(),
    };
    let esp_node = FsNode::Container {
        attr: dir_attr.clone(),
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
    .with_label("BOOT")
    .with_uuid("ABCD-1234")
    .with_root(esp_node);

    let data_file = FsNode::File {
        name: "test.dat".into(),
        source: Box::new(rimio::VecRimIO::new(vec![0xAB; 1024])),
        attr: file_attr,
    };
    let data_node = FsNode::Container {
        attr: dir_attr,
        children: vec![data_file],
    };

    let data_part = Partition::new(
        "DATA",
        PartitionKind::Data,
        Filesystem::Ext4,
        40960, // 20 MiB
        guid_gen.generate_guid(),
    )
    .with_label("DATA")
    .with_uuid("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
    .with_root(data_node);

    layout = layout.add_partition(esp_part).add_partition(data_part);
    (layout, guid_gen)
}

#[test]
fn test_identical_engine_equivalence() {
    let total_size = 64 * 1024 * 1024; // 64 MiB

    let (mut layout1, _) = make_layout(0x4242_4242);
    let mut buf1 = vec![0u8; total_size];
    let mut io1 = MemRimIO::new(&mut buf1);
    build_on_io(&mut layout1, &mut io1, |_| {}).expect("build 1 failed");

    let (mut layout2, _) = make_layout(0x4242_4242);
    let mut buf2 = vec![0u8; total_size];
    let mut io2 = MemRimIO::new(&mut buf2);
    build_on_io(&mut layout2, &mut io2, |_| {}).expect("build 2 failed");

    // Byte-for-byte comparison with compact error output
    if buf1 != buf2 {
        let mut diff_count = 0;
        for (i, (&b1, &b2)) in buf1.iter().zip(buf2.iter()).enumerate() {
            if b1 != b2 {
                if diff_count < 10 {
                    eprintln!(
                        "Diff at byte 0x{:X} (LBA {}): 0x{:02X} != 0x{:02X}",
                        i,
                        i / 512,
                        b1,
                        b2
                    );
                }
                diff_count += 1;
            }
        }
        panic!(
            "Deterministic builds produced {} differing bytes!",
            diff_count
        );
    }
}
