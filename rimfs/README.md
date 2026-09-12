# rimfs

[![crates.io](https://img.shields.io/crates/v/rimfs.svg)](https://crates.io/crates/rimfs)
[![Documentation](https://docs.rs/rimfs/badge.svg)](https://docs.rs/rimfs)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs`** is the unified public facade crate for the **[RIM](../README.md)** filesystem ecosystem (Layer 3).

It re-exports all seven filesystem and archive engines alongside shared abstractions from [`rimfs-core`](../rimfs-core), providing a standardized, feature-gated API for formatting, injecting, resolving, and verifying filesystems in pure Rust.

---

## Architecture & Subsystem Mapping

`rimfs` coordinates seven specialized storage engines:

| Engine Module | Driver Crate | Target Format & Capabilities |
|---|---|---|
| **`rimfs::fat`** | [`rimfs-fat`](../rimfs-fat) | FAT12, FAT16, FAT32 (DOS/Windows standard) and the 64-bit **RimFAT** integrity extension. |
| **`rimfs::exfat`** | [`rimfs-exfat`](../rimfs-exfat) | ExFAT flash storage format with Allocation Bitmap and UpCase table. |
| **`rimfs::ext`** | [`rimfs-ext`](../rimfs-ext) | Linux native Ext2, Ext3, and Ext4 with 48-bit physical extent trees and BGDT. |
| **`rimfs::ntfs`** | [`rimfs-ntfs`](../rimfs-ntfs) | Pure-Rust NTFS 3.1 with $MFT, USA fixups, INDX B-tree directories, and `$Secure`. |
| **`rimfs::iso`** | [`rimfs-iso`](../rimfs-iso) | ISO 9660 Level 1-3, Joliet (UTF-16), Rock Ridge (POSIX), and El Torito hybrid bootloader. |
| **`rimfs::tar`** | [`rimfs-tar`](../rimfs-tar) | POSIX UStar streaming archive driver (`no_std + alloc`). |
| **`rimfs::zip`** | [`rimfs-zip`](../rimfs-zip) | PKWARE ZIP & ZIP64 streaming archive driver with Store (0) and Deflate (8). |

For complete technical specifications of each engine, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs
```

To enable only specific drivers (e.g. FAT and EXT for an embedded Linux boot image):

```bash
cargo add rimfs --no-default-features --features fat,ext,alloc
```

---

## Usage Example: Formatting & Injecting into an Ext4 Filesystem

```rust
use rimfs::core::resolver::attr::FileAttributes;
use rimfs::core::resolver::node::FsNode;
use rimfs::ext::{ExtFormatter, ExtInjector, ExtMeta};
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate block storage (RAM buffer or file)
    let mut storage = vec![0u8; 64 * 1024 * 1024]; // 64 MiB
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge filesystem metadata
    let meta = ExtMeta::new(storage.len() as u64, Some("ROOTFS"))?;

    // 3. Format the volume layout
    ExtFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Construct payload file tree
    let mut attr = FileAttributes::new_file();
    attr.mode = Some(0o100644);
    attr.uid = Some(1000);
    attr.gid = Some(1000);

    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file_from_source(
        "hello.txt",
        Box::new(rimio::VecRimIO::new(b"Hello from pure-Rust Ext4!\n".to_vec())),
        attr,
    );
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }

    // 5. Inject files and directory structures
    let mut injector = ExtInjector::new(&mut disk, &meta)?;
    injector.inject_tree(&mut tree)?;

    println!("Successfully formatted and injected Ext4 image in userspace!");

    Ok(())
}
```

---

## Cargo Features

- **`fat`** (default): Enables FAT12/16/32 and RimFAT support.
- **`exfat`** (default): Enables ExFAT support.
- **`ext`** (default): Enables EXT2/3/4 support.
- **`ntfs`** (default): Enables NTFS 3.1 support.
- **`iso`** (default): Enables ISO 9660, Joliet, Rock Ridge, and El Torito.
- **`tar`** (default): Enables POSIX UStar TAR archive support.
- **`zip`** (default): Enables PKWARE ZIP & ZIP64 support.
- **`std`** (default): Enables host OS filesystem walking (`StdResolver`).
- **`alloc`**: Enables heap-dependent features in `no_std` environments.
- **`uefi`**: Enables bare-metal UEFI firmware integration.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
