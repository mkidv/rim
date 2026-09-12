# rimfs-exfat

[![crates.io](https://img.shields.io/crates/v/rimfs-exfat.svg)](https://crates.io/crates/rimfs-exfat)
[![Documentation](https://docs.rs/rimfs-exfat/badge.svg)](https://docs.rs/rimfs-exfat)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-exfat`** is the dedicated ExFAT filesystem driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides pure-Rust formatting, file injection, path resolution, and consistency checking for modern high-capacity flash storage (>32 GiB) without requiring external C libraries or kernel modules.

---

## Features & Capabilities

- **Full ExFAT Specification Compliance**: Main and Backup Boot Regions (12 sectors each) protected by rotational sum boot checksums.
- **Allocation Bitmap**: Single-bit cluster allocation tracking (0 = free, 1 = used), enabling fast contiguous allocations via `NoFatChain` flags.
- **UpCase Table**: Pre-compiled Unicode uppercase mapping table supporting run-length compression for case-insensitive filename comparison.
- **Directory Entry Sets**: High-fidelity construction of 32-byte directory sets (Primary File Entry `0x85`, Stream Extension Entry `0xC0`, and File Name Entries `0xC1` supporting up to 255 UTF-16 code units).
- **Pipeline Components**:
  - `ExFatFormatter`: Writes boot sectors, OEM parameter blocks, Allocation Bitmap, UpCase table, and root directory.
  - `ExFatAllocator`: Manages cluster allocations and bitmap synchronization.
  - `ExFatInjector`: Ingests files, directory hierarchies, and timestamps.
  - `ExFatResolver`: Path resolution, attribute lookups, and streaming data reading.
  - `ExFatChecker`: Validates VBR checksums, bitmap synchronization, and directory entry sets.

For deep technical specifications on boot checksums and directory sets, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-exfat
```

For constrained `no_std` environments:

```bash
cargo add rimfs-exfat --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::node::FsNode;
use rimfs_exfat::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Prepare storage medium (e.g. 64 MiB buffer)
    let mut storage = vec![0u8; 64 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge ExFAT volume metadata
    let meta = ExFatMeta::new(storage.len() as u64, Some("FLASH_DISK"))?;

    // 3. Format ExFAT volume structures
    ExFatFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject payload directory tree
    let mut injector = ExFatInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file("data.bin", b"Payload stored in ExFAT".to_vec());
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }
    injector.inject_tree(&mut tree)?;

    // 5. Run structural verification
    let checker = ExFatChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("ExFAT volume verified cleanly: {} clusters allocated", report.clusters_allocated);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`alloc`**: Enables dynamic allocation for directory sets and UpCase tables.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
