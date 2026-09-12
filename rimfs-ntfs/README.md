# rimfs-ntfs

[![crates.io](https://img.shields.io/crates/v/rimfs-ntfs.svg)](https://crates.io/crates/rimfs-ntfs)
[![Documentation](https://docs.rs/rimfs-ntfs/badge.svg)](https://docs.rs/rimfs-ntfs)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-ntfs`** is the pure-Rust NTFS 3.1 filesystem driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides rootless userspace formatting, injection, resolution, and verification for Windows NT / 2000 / XP / 7 / 10 / 11 NTFS volumes without requiring external C libraries (such as `ntfs-3g`) or kernel drivers.

---

## Features & Capabilities

- **Master File Table ($MFT)**: High-fidelity generation and parsing of 1024-byte `FILE` records, Update Sequence Array (USA/Fixup) torn write protection, and record attribute chaining.
- **Attributes Pipeline**:
  - Resident and Non-Resident `$DATA` attributes with variable-length runlist compression and fragmented cluster run encoding/decoding.
  - `$STANDARD_INFORMATION` (timestamps, DOS file permissions).
  - `$FILE_NAME` (Win32, DOS, and POSIX namespaces).
- **Directory B-Tree Indexing**: Index Root (`$INDEX_ROOT`), Index Allocation (`$INDEX_ALLOCATION`), and `$BITMAP` records forming balanced B-trees of 4096-byte `INDX` blocks for scalable lookups.
- **Security Descriptors (`$Secure`)**: Complete Security ID database management with `$SDS` (raw security descriptors), `$SDH` (hash B-tree), and `$SII` (security ID index) streams.
- **System Metas**: `$Boot` sector, `$MFT`, `$MFTMirr`, `$LogFile`, `$Volume`, `$AttrDef`, `$Bitmap`, `$BadClust`, and non-resident `$UpCase`.
- **Pipeline Components**:
  - `NtfsFormatter`: Initializes boot sectors, system MFT records, and security streams.
  - `NtfsAllocator`: Manages cluster allocations via the volume cluster bitmap.
  - `NtfsInjector`: Ingests files, directories, and data streams into the MFT.
  - `NtfsResolver`: MFT traversal, directory B-tree lookups, and attribute parsing.
  - `NtfsChecker`: Verifies boot sector parameters, MFT signatures, USA fixups, and system record integrity.

For deep technical specifications on USA fixups, MFT record layouts, and `$Secure`, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-ntfs
```

For constrained `no_std` environments:

```bash
cargo add rimfs-ntfs --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::node::FsNode;
use rimfs_ntfs::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate block storage (e.g. 128 MiB volume buffer)
    let mut storage = vec![0u8; 128 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge NTFS volume metadata
    let meta = NtfsMeta::new(storage.len() as u64, Some("WINDOWS"))?;

    // 3. Format NTFS volume layout ($Boot, $MFT, $Secure, etc.)
    NtfsFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject payload directory tree
    let mut injector = NtfsInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file("ntfs_file.txt", b"Pure-Rust NTFS injection\n".to_vec());
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }
    injector.inject_tree(&mut tree)?;

    // 5. Verify volume reachability and MFT record integrity
    let checker = NtfsChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("NTFS volume verified cleanly: {} records in MFT", report.records_checked);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`alloc`**: Enables heap-dependent MFT records and B-tree directory indexing.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
