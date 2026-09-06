extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;

use rimfs::core::resolver::{FileAttributes, FsNode};
use rimfs::core::time::OffsetDateTime;
use rimgen::guid::{GuidGenerator, SeededGuidGenerator};
use rimgen::layout::{Filesystem, Layout, Partition, PartitionKind};
use rimio::VecRimIO;

pub const DEMO_IMAGE_SIZE_BYTES: usize = 64 * 1024 * 1024; // 64 MiB
pub const DEMO_SEED: u64 = 0x5741_534D_5249_4D31; // "WASMRIM1"

/// Creates the deterministic 64 MiB demo layout with FAT32 (ESP) and EXT4 (rootfs).
pub fn make_demo_layout() -> (Layout<'static>, SeededGuidGenerator) {
    let mut guid_gen = SeededGuidGenerator::new(DEMO_SEED);
    let mut layout = Layout::new(guid_gen.generate_guid());

    // Fixed timestamps for bit-exact deterministic reproducibility
    let mut file_attr = FileAttributes::new_file();
    file_attr.created = Some(OffsetDateTime::UNIX_EPOCH);
    file_attr.modified = Some(OffsetDateTime::UNIX_EPOCH);
    file_attr.accessed = Some(OffsetDateTime::UNIX_EPOCH);

    let mut dir_attr = FileAttributes::new_dir();
    dir_attr.created = Some(OffsetDateTime::UNIX_EPOCH);
    dir_attr.modified = Some(OffsetDateTime::UNIX_EPOCH);
    dir_attr.accessed = Some(OffsetDateTime::UNIX_EPOCH);

    // -------------------------------------------------------------
    // Partition 1: ESP (FAT32, 32 MiB)
    // -------------------------------------------------------------
    let readme_content = b"========================================\n\
RIM WebAssembly Engine Demo\n\
Synthesized entirely client-side in browser!\n\
========================================\n\
Disk size: 64 MiB\n\
Partitions: GPT [FAT32 (ESP) + EXT4 (rootfs)]\n\
Engine: Pure Rust (no_std + alloc)\n";

    let readme_file = FsNode::File {
        name: "README.TXT".into(),
        source: Box::new(VecRimIO::new(readme_content.to_vec())),
        attr: file_attr.clone(),
    };

    let esp_root = FsNode::Container {
        attr: dir_attr.clone(),
        children: vec![readme_file],
    };

    let esp_part = Partition::new(
        "ESP",
        PartitionKind::Esp,
        Filesystem::Fat32,
        65536, // 32 MiB (65536 sectors @ 512B)
        guid_gen.generate_guid(),
    )
    .with_bootable(true)
    .with_label("BOOT")
    .with_uuid("ABCD-1234")
    .with_root(esp_root);

    // -------------------------------------------------------------
    // Partition 2: rootfs (EXT4, 24 MiB)
    // -------------------------------------------------------------
    let hello_content = b"Hello from EXT4 generated inside WebAssembly!\n";
    let hello_file = FsNode::File {
        name: "hello.txt".into(),
        source: Box::new(VecRimIO::new(hello_content.to_vec())),
        attr: file_attr.clone(),
    };

    let conf_content =
        b"# RIM Browser Synthesizer Configuration\nenabled=true\nmode=client-side\ntarget=wasm32\n";
    let conf_file = FsNode::File {
        name: "rim.conf".into(),
        source: Box::new(VecRimIO::new(conf_content.to_vec())),
        attr: file_attr,
    };

    let etc_dir = FsNode::Dir {
        name: "etc".into(),
        children: vec![conf_file],
        attr: dir_attr.clone(),
    };

    let mut symlink_attr = FileAttributes::new_symlink();
    symlink_attr.created = Some(OffsetDateTime::UNIX_EPOCH);
    symlink_attr.modified = Some(OffsetDateTime::UNIX_EPOCH);
    symlink_attr.accessed = Some(OffsetDateTime::UNIX_EPOCH);

    let hello_link = FsNode::Symlink {
        name: "hello_link".into(),
        target: "hello.txt".into(),
        attr: symlink_attr,
    };

    let ext_root = FsNode::Container {
        attr: dir_attr,
        children: vec![hello_file, etc_dir, hello_link],
    };

    let ext_part = Partition::new(
        "rootfs",
        PartitionKind::Linux,
        Filesystem::Ext4,
        49152, // 24 MiB (49152 sectors @ 512B)
        guid_gen.generate_guid(),
    )
    .with_label("ROOTFS")
    .with_uuid("12345678-1234-5678-1234-567812345678")
    .with_root(ext_root);

    layout = layout.add_partition(esp_part).add_partition(ext_part);
    (layout, guid_gen)
}
