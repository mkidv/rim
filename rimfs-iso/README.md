# rimfs-iso

[![crates.io](https://img.shields.io/crates/v/rimfs-iso.svg)](https://crates.io/crates/rimfs-iso)
[![Documentation](https://docs.rs/rimfs-iso/badge.svg)](https://docs.rs/rimfs-iso)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimfs-iso`** is the pure-Rust ISO 9660 optical disk and hybrid image filesystem driver of the **[RIM](../README.md)** ecosystem (Layer 3).

It provides complete generation, extraction, and validation for ISO 9660 images supporting **Joliet** (Unicode UTF-16), **Rock Ridge** (POSIX permissions and symlinks), and **El Torito** (hybrid BIOS & UEFI booting) without requiring external utilities like `mkisofs` or `xorriso`.

---

## Features & Capabilities

- **ISO 9660 (ECMA-119)**: 2048-byte sector geometry with ISO 733 and 723 both-endian integers.
- **Pre-Computed Planning Architecture**: Calculates all LBA assignments and descriptor layouts prior to sequential serialization, ensuring zero backtracking and optimal performance under WebAssembly and UEFI environments.
- **Joliet Extension**: Unicode UTF-16 Big-Endian filenames and directory hierarchies for Windows compatibility.
- **Rock Ridge (SUSP) Extension**: Full POSIX support for Unix file modes (`PX`), alternate long names (`NM`), symbolic links (`SL`), and UIDs/GIDs.
- **El Torito Hybrid Boot Specification**:
  - BIOS No-Emulation bootloader support.
  - **UEFI Boot Support**: Automatic in-memory synthesis of FAT12/16/32 EFI System Partition boot images (containing `/EFI/BOOT/BOOTX64.EFI`) using `rimfs-fat`.
- **Pure `no_std + alloc` & Zero Subprocesses**: Creates bootable ISO images directly in memory or streams.

For deep technical details on Volume Descriptors, SUSP records, and El Torito boot catalogs, see **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimfs-iso
```

For constrained `no_std` environments:

```bash
cargo add rimfs-iso --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimfs_core::resolver::node::FsNode;
use rimfs_iso::prelude::*;
use rimio::prelude::MemRimIO;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Allocate buffer (e.g. 32 MiB optical disk buffer)
    let mut storage = vec![0u8; 32 * 1024 * 1024];
    let mut disk = MemRimIO::new(&mut storage);

    // 2. Converge ISO 9660 metadata
    let meta = IsoMeta::new(storage.len() as u64, Some("BOOT_ISO"))?;

    // 3. Format volume descriptors (PVD, Joliet SVD, El Torito)
    IsoFormatter::new(&mut disk, &meta).format(false)?;

    // 4. Inject payload directory tree
    let mut injector = IsoInjector::new(&mut disk, &meta)?;
    let mut tree = FsNode::new_dir("/");
    let file = FsNode::new_file("readme.txt", b"Pure-Rust ISO 9660 image\n".to_vec());
    if let FsNode::Dir { ref mut children, .. } = tree {
        children.push(file);
    }
    injector.inject_tree(&mut tree)?;

    // 5. Verify ISO structural integrity
    let checker = IsoChecker::new(&mut disk, &meta);
    let report = checker.verify_reachability()?;
    println!("ISO volume verified cleanly: {} sectors checked", report.sectors_checked);

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables standard library integrations.
- **`alloc`**: Enables heap-dependent volume descriptor calculation and directory trees.

---

## Related Documentation

- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
