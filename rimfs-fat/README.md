# rimfs-fat

`rimfs-fat` is the dedicated FAT filesystem driver of the RIM ecosystem, providing robust support for **FAT12**, **FAT16**, **FAT32**, and the 64-bit optimized **RimFAT** extension.

## Features

- **FAT12 / FAT16 / FAT32**: Standard DOS/Windows compliant formatting, cluster chain allocation, and VBR/FSInfo synthesis.
- **RimFAT**: 64-bit volume extension designed for large storage and fast streaming without FAT32 cluster limitations (see `RIMFAT_SPEC.md`).
- **Long File Names (LFN)**: Full UTF-16 Long File Name generation and checksum validation alongside Short 8.3 File Names (SFN).
- **Pipeline Components**:
  - `FatFormatter`: Initializes boot sectors, FAT tables, FSInfo, and root directories.
  - `FatAllocator`: Cluster chain allocation and traversal.
  - `FatInjector`: Recursive streaming directory and file writer.
  - `FatResolver`: Path-to-cluster resolution and metadata lookup.
  - `FatChecker`: Consistency checker for boot parameters, FAT table chains, cross-linked clusters, and orphan entries.

## Usage

```toml
[dependencies]
rimfs-fat = { version = "0.6.3", default-features = false, features = ["std"] }
```

```rust
use rimfs_fat::prelude::*;
use rimio::StdRimIO;
use std::fs::File;

let mut file = File::options().read(true).write(true).open("fat32.img")?;
let mut disk = StdRimIO::new(&mut file);
let meta = FatMeta::new(disk.len(), Some("EFI_SYSTEM"));

// Format partition as FAT32
FatFormatter::new(&mut disk, &meta).format()?;
```

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
