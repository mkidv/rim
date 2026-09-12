# rimfs-fat

[![crates.io](https://img.shields.io/crates/v/rimfs-fat.svg)](https://crates.io/crates/rimfs-fat)
[![Documentation](https://docs.rs/rimfs-fat/badge.svg)](https://docs.rs/rimfs-fat)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-fat`** is the dedicated FAT filesystem driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides pure-Rust formatting, injection, resolution, and verification for **FAT12**, **FAT16**, **FAT32**, and the 64-bit optimized **RimFAT** integrity extension.

---

## Features & Capabilities

- **FAT12 / FAT16 / FAT32**: Full Microsoft FAT specification compliance, Volume Boot Record (VBR), FSInfo sector, FAT1/FAT2 tables, and cluster chain traversal.
- **RimFAT Integrity Extension**: 64-bit nanosecond timestamps and volume metadata checksumming stored in reserved BPB/FSInfo areas with complete backward read compatibility for standard FAT drivers.
- **Long File Names (LFN)**: UTF-16 VFAT long filenames with automatic 8.3 Short File Name (SFN) alias generation and checksum binding.
- **Pipeline Components**:
  - `FatFormatter`: Writes VBR, FSInfo, FAT tables, and initial root directory.
  - `FatAllocator`: Cluster chain allocation and free cluster tracking.
  - `FatInjector`: Streaming file, directory, and attribute ingestion.
  - `FatResolver`: Path-to-cluster resolution, directory walking, and file streaming.
  - `FatChecker`: Consistency validation for boot parameters, FAT table chains, cross-linked clusters, and orphan detection.

For deep technical details on BPB layouts, cluster chains, and RimFAT specifications, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-fat
```

For constrained `no_std` environments:

```bash
cargo add rimfs-fat --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::node::FsNode;
use rimfs_fat::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate block storage (e.g. 64 MiB partition buffer)
    let mut storage = vec![0u8; 64 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge FAT32 geometry metadata
    let meta = FatMeta::new_fat32(storage.len() as u64, Some("ESP"))?;

    // 3. Format FAT32 filesystem structures
    FatFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject directory tree with payload files
    let mut injector = FatInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file("BOOTX64.EFI", b"UEFI payload binary data".to_vec());
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }
    injector.inject_tree(&mut tree)?;

    // 5. Verify volume reachability and ensure zero orphan clusters
    let checker = FatChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("FAT32 volume verified cleanly: {} clusters allocated", report.clusters_allocated);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`alloc`**: Enables heap-dependent directory trees and string handling.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
