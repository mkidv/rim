# rimimg

[![crates.io](https://img.shields.io/crates/v/rimimg.svg)](https://crates.io/crates/rimimg)
[![Documentation](https://docs.rs/rimimg/badge.svg)](https://docs.rs/rimimg)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimimg`** is the virtual machine disk container and image format engine of the **[RIM](../README.md)** ecosystem (Layer 1).

It provides pure-Rust detection, encapsulation, decapsulation, streaming conversion, and transparent logical I/O adapters for all major virtual disk formats without requiring external hypervisors or C toolchains like `qemu-img`.

---

## Supported Virtual Disk Formats

| Format | Extension | Type & Capabilities |
|---|---|---|
| **RAW** | `.img`, `.raw` | Direct 1:1 sector mapping, zero overhead (Read & Write). |
| **Microsoft VHD** | `.vhd` | Fixed-size VHD with dynamic 512-byte `conectix` footer, CHS geometry, and UUID (Read & Write). |
| **VMware VMDK** | `.vmdk` | Monolithic Flat descriptor header (`# Disk DescriptorFile`) with linear extent mapping (Read & Write). |
| **VirtualBox VDI** | `.vdi` | Fixed VDI 1.1 format with pre-allocated block tables and sector alignment (Read & Write). |
| **QEMU QCOW2** | `.qcow2` | **Full Dynamic Sparse Allocator** (v2 & v3), two-level indexing (L1 $\to$ L2 $\to$ clusters), dynamic growth, zero-cluster optimization, and lazy L2 table flushing (Read & Write). |

---

## Architecture & System Role

`rimimg` bridges the gap between low-level storage streams ([`rimio`](../rimio)) and high-level filesystem engines ([`rimfs-*`](../rimfs)) or partitioning tools ([`rimpart`](../rimpart)):

- **Transparent Logical I/O Adapters**: [`create_image_io`](src/io.rs) and [`open_image_io`](src/io.rs) return an [`ImageIO`](src/io.rs) handle that implements [`RimIO`](../rimio). Filesystem injectors and partitioners can format and write directly into a `.qcow2` or `.vhd` file as if it were a flat raw disk.
- **Dynamic Sparse Allocation**: The QCOW2 driver allocates clusters on demand as writes occur, leaving unwritten space sparse on host disks while reporting accurate virtual capacity.
- **Zero-Copy Streaming Conversion**: [`wrap_io`](src/convert.rs) and [`unwrap_io`](src/convert.rs) translate between raw data and containers with zero temporary disk files.

For deep technical details on QCOW2 indexing and container footers, see **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimimg
```

For constrained `no_std` environments:

```bash
cargo add rimimg --no-default-features --features alloc
```

---

## Usage Examples

### 1. Formatting and Writing Directly into a QCOW2 Image

```rust
use rimimg::{create_image_io, ImageFormat, ImageOptions};
use rimio::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Underlying storage file or buffer
    let mut storage_buffer = vec![0u8; 10 * 1024 * 1024]; // 10 MiB container buffer
    let mut dest_io = MemRimIO::new(&mut storage_buffer);

    // Initialize a 50 MiB virtual disk inside the QCOW2 container
    let virtual_disk_size = 50 * 1024 * 1024;
    let mut virtual_disk = create_image_io(
        &mut dest_io,
        virtual_disk_size,
        ImageFormat::Qcow2,
        ImageOptions::default(),
    )?;

    // virtual_disk implements RimIO! Write anywhere within virtual space:
    virtual_disk.write_at(0, b"EFI PART / Partition table placeholder")?;
    virtual_disk.flush()?;

    println!("Virtual disk capacity: {} bytes", virtual_disk.total_size()?);

    Ok(())
}
```

### 2. Auto-Detecting and Opening a Virtual Disk Container

```rust
use rimimg::open_image_io;
use rimio::prelude::*;
use std::fs::OpenOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("disk.vhd")?;
    let mut disk_io = StdRimIO::new(&mut file);

    // Automatically inspects signatures (conectix, QFI\xfb, VMDK descriptor, etc.)
    let mut virtual_disk = open_image_io(&mut disk_io)?;
    
    let mut sector = [0u8; 512];
    virtual_disk.read_at(0, &mut sector)?;
    println!("Successfully read LBA 0 through container decoder");

    Ok(())
}
```

---

## Cargo Features

- **`std`** (default): Enables host OS file operations and error integration.
- **`alloc`**: Enables dynamic cluster allocation for QCOW2 and container wrapping/unwrapping buffers.

---

## Related Documentation

- **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
