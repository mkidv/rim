# rimfs-ext

`rimfs-ext` is the dedicated EXT filesystem driver of the RIM ecosystem, supporting **Ext2**, **Ext3**, and **Ext4** in pure Rust.

## Features

- **Ext2 / Ext3 / Ext4**: Supports classic block maps (direct, indirect, doubly indirect, triply indirect) and modern 48-bit physical extent trees (`ext4`).
- **Superblock & Group Descriptors**: Block Group Descriptor Tables (BGDT), flex block groups, sparse superblocks, 64-bit feature flags, and checksums.
- **Inode & Directory Layout**: Inodes with timestamps, extended attributes, and directory entries with file types (`EXT4_FEATURE_INCOMPAT_FILETYPE`).
- **Pipeline Components**:
  - `ExtFormatter`: Formats partitions with Linux-kernel / `e2fsprogs` compatibility.
  - `ExtAllocator`: Manages block bitmaps and inode allocation across block groups.
  - `ExtInjector`: Injects files, directories, and symlinks.
  - `ExtResolver`: Inode traversal, extent leaf decoding, and path lookups.
  - `ExtChecker`: Verifies superblock magic, BGDT consistency, bitmap synchronization, and directory structure.

## Usage

```toml
[dependencies]
rimfs-ext = { version = "0.6.0", default-features = false, features = ["std"] }
```

```rust
use rimfs_ext::prelude::*;
use rimio::StdRimIO;
use std::fs::File;

let mut file = File::options().read(true).write(true).open("ext4.img")?;
let mut disk = StdRimIO::new(&mut file);
let meta = ExtMeta::new(disk.len(), Some("ROOTFS"));

ExtFormatter::new(&mut disk, &meta).format()?;
```

