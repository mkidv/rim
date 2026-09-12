# rimfs-zip

[![crates.io](https://img.shields.io/crates/v/rimfs-zip.svg)](https://crates.io/crates/rimfs-zip)
[![Documentation](https://docs.rs/rimfs-zip/badge.svg)](https://docs.rs/rimfs-zip)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-zip`** is the dedicated ZIP archive filesystem driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides `no_std + alloc` streaming creation, extraction, resolution, and verification of ZIP and ZIP64 archives conforming to the standard `rimfs` filesystem pipeline.

---

## Features & Capabilities

- **Standard ZIP & ZIP64 Compliant**: Supports Local File Headers (LFH), Central Directory (CD), End of Central Directory (EOCD), and ZIP64 extensions for archives $> 4\text{ GiB}$ or $> 65,535$ entries.
- **Stream-First Writing**: Streams file payloads directly into `RimIO` without caching full files in RAM, calculating CRC-32 on-the-fly.
- **Compression Algorithms**: Supports **Store** (method 0, uncompressed, `#![no_std]` friendly) and **Deflate** (method 8, streaming compression).
- **Extended Unix & POSIX Metadata**: Full support for Extended Timestamps (`0x5455`), Info-ZIP Unix UID/GID (`0x7875`), POSIX file modes, directory records, and symlinks.
- **Embedded & Bare-Metal (`no_std + alloc`)**: Fully operational in constrained environments without `std`.
- **Pipeline Components**:
  - `ZipFormatter`: Initializes empty archive structures.
  - `ZipAllocator`: Stream offset allocation tracking.
  - `ZipInjector`: Streaming file, directory, and symlink writer.
  - `ZipResolver`: In-place Central Directory traversal, path resolution, and file extraction.
  - `ZipChecker`: Consistency validation for headers, central directory records, and CRC-32 checksums.

For deep technical details on Central Directory structures and ZIP64 extensions, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-zip
```

For constrained `no_std` environments:

```bash
cargo add rimfs-zip --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::node::FsNode;
use rimfs_zip::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate buffer (e.g. 10 MiB buffer)
    let mut storage = vec![0u8; 10 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge ZIP metadata
    let meta = ZipMeta::new(storage.len() as u64, Some("BACKUP"))?;

    // 3. Initialize ZIP structure
    ZipFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject payload directory tree
    let mut injector = ZipInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file("archive_entry.txt", b"Streamed ZIP entry data\n".to_vec());
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }
    injector.inject_tree(&mut tree)?;
    injector.flush()?;

    // 5. Verify ZIP archive consistency
    let checker = ZipChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("ZIP archive verified cleanly: {} records checked", report.records_checked);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`deflate`** (default): Enables Deflate compression support via `flate2`.
- **`alloc`**: Enables heap-dependent Central Directory indexing and tree nodes.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
