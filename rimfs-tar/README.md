# rimfs-tar

[![crates.io](https://img.shields.io/crates/v/rimfs-tar.svg)](https://crates.io/crates/rimfs-tar)
[![Documentation](https://docs.rs/rimfs-tar/badge.svg)](https://docs.rs/rimfs-tar)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-tar`** is the dedicated POSIX UStar TAR archive driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides `no_std + alloc` streaming creation, extraction, resolution, and validation of TAR archives conforming to the standard `rimfs` filesystem pipeline.

---

## Features & Capabilities

- **POSIX UStar Compliant**: Standard 512-byte header serialization, octal field encoding, header checksum validation, and end-of-archive framing (`2 x 512` zero blocks).
- **Stream-First Writing**: Directly streams payloads into `RimIO` block streams without requiring full-file buffering in RAM.
- **Full Metadata Preservation**: Preserves POSIX file modes, UID/GID, timestamps, file names (with standard `prefix` splitting for long paths), and symbolic links.
- **Embedded & Bare-Metal (`no_std + alloc`)**: Operates seamlessly in constrained environments without `std`.
- **Pipeline Components**:
  - `TarFormatter`: Initializes archive header structures.
  - `TarAllocator`: 512-byte block allocation tracking.
  - `TarInjector`: Streaming file, directory, and symlink writer into TAR archives.
  - `TarResolver`: In-place archive traversal, path resolution, and file extraction.
  - `TarChecker`: Consistency validation for headers, octal numbers, checksums, and trailer records.

For deep technical details on UStar headers and block padding, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-tar
```

For constrained `no_std` environments:

```bash
cargo add rimfs-tar --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::node::FsNode;
use rimfs_tar::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate buffer (e.g. 10 MiB buffer)
    let mut storage = vec![0u8; 10 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge TAR metadata
    let meta = TarMeta::new(storage.len() as u64, Some("ARCHIVE"))?;

    // 3. Initialize TAR structure
    TarFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject directory tree into archive
    let mut injector = TarInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file("log.txt", b"Streamed archive entry\n".to_vec());
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }
    injector.inject_tree(&mut tree)?;
    injector.flush()?;

    // 5. Verify archive consistency
    let checker = TarChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("TAR archive verified cleanly: {} records checked", report.records_checked);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`alloc`**: Enables heap-dependent directory trees and stream buffering.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
