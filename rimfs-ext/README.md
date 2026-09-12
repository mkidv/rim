# rimfs-ext

[![crates.io](https://img.shields.io/crates/v/rimfs-ext.svg)](https://crates.io/crates/rimfs-ext)
[![Documentation](https://docs.rs/rimfs-ext/badge.svg)](https://docs.rs/rimfs-ext)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-ext`** is the pure-Rust Linux native EXT filesystem driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides complete formatting, injection, resolution, and verification for **Ext2**, **Ext3**, and modern **Ext4** filesystems, including 48-bit physical extent trees and Block Group Descriptor Tables (BGDT).

---

## Features & Capabilities

- **Ext2 / Ext3 / Ext4**: Supports classic block mapping (direct, single indirect, doubly indirect, triply indirect) and modern extent trees (`EXT4_FEATURE_INCOMPAT_EXTENTS`).
- **Extent Tree Architecture**: Implements a balanced B-tree of 12-byte extent structures (`ext4_extent_header`, `ext4_extent_idx`, and `ext4_extent`), addressing up to $2^{48}$ physical blocks with up to 32,768 contiguous blocks per leaf record.
- **Inodes & POSIX Semantics**: Standard 256-byte inodes with nanosecond timestamps, 32-bit UID/GID, full 12-bit POSIX modes (`setuid`, `setgid`, `sticky`), and directory entries with file type indicators.
- **Symlink Optimization**: Fast inline symlinks ($\le 60$ bytes stored directly inside the inode's block pointer array, consuming zero data blocks) alongside extent-allocated slow symlinks.
- **Pipeline Components**:
  - `ExtFormatter`: Writes the superblock at offset 1024, BGDT, block/inode bitmaps, and inode tables.
  - `ExtAllocator`: Manages block allocation across flex block groups.
  - `ExtInjector`: Recursively writes files, directories, symlinks, and POSIX permissions.
  - `ExtResolver`: Inode traversal, extent decoding, path resolution, and symlink reading.
  - `ExtChecker`: Consistency verification for superblocks, BGDT synchronization, and reachability.

For deep technical details on extent trees and BGDT layouts, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-ext
```

For constrained `no_std` environments:

```bash
cargo add rimfs-ext --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::attr::FileAttributes;
use rimfs_core::resolver::node::FsNode;
use rimfs_ext::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate block storage (e.g. 64 MiB buffer)
    let mut storage = vec![0u8; 64 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge Ext4 volume metadata
    let meta = ExtMeta::new(storage.len() as u64, Some("ROOTFS"))?;

    // 3. Format Ext4 structures (superblock, BGDT, inode table)
    ExtFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject directory hierarchy with POSIX permissions
    let mut injector = ExtInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");

    let mut attr = FileAttributes::new_file();
    attr.mode = Some(0o100755); // rwxr-xr-x executable
    attr.uid = Some(1000);
    attr.gid = Some(1000);

    let script = FsNode::new_file_from_source(
        "init.sh",
        Box::new(rimio::VecRimIO::new(b"#!/bin/sh\necho 'Booting pure-Rust Ext4'\n".to_vec())),
        attr,
    );
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(script);
    }
    injector.inject_tree(&mut tree)?;

    // 5. Verify filesystem consistency
    let checker = ExtChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("Ext4 volume verified: {} inodes allocated", report.inodes_allocated);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`alloc`**: Enables heap-dependent extent tree nodes and directory buffers.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
