# rimgen

[![crates.io](https://img.shields.io/crates/v/rimgen.svg)](https://crates.io/crates/rimgen)
[![Documentation](https://docs.rs/rimgen/badge.svg)](https://docs.rs/rimgen)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![Live Demo](https://img.shields.io/badge/Live%20Demo-mki.dev%2Frim-ff4081?style=flat-square&logo=googlechrome&logoColor=white)](https://mki.dev/rim)

**`rimgen`** is the declarative storage layout synthesis engine of the **[RIM](../README.md)** ecosystem (Layer 4).

It transforms declarative TOML configuration files into complete, bootable virtual disks or physical media images without requiring root (`sudo`) privileges, loop devices, or external filesystem utilities.

> 🚀 **Try `rimgen` live in your browser:** [mki.dev/rim](https://mki.dev/rim) — Interactive layout playground with client-side WebAssembly synthesis and in-browser UEFI VM demo!

---

## Features & Capabilities

- **Pure Rust & 100% Rootless**: Runs in userspace on Linux, macOS, and Windows with identical, deterministic output.
- **Direct Stream Synthesis (`build_config_on_io`)**: Synthesizes disk images directly into any [`RimIO`](../rimio) stream (RAM buffers, raw files, VM disk containers like QCOW2/VHD, or UEFI blocks) with zero temporary disk files.
- **Automated Geometry & Alignment**: Enforces partition alignment (default: `1 MiB` for flash/SSD optimization) and automatically sizes partitions (`size = "auto"`) based on payload file tree volume plus filesystem metadata overhead.
- **Typed Event Notifications (`BuildEvent`)**: Real-time event hooks for layout planning, GPT writes, partition formatting, and payload progress.

For deep technical specifications on alignment algorithms, auto-sizing math, and multi-stage build pipelines, see **[Storage Synthesis & Transfer Engine](../docs/SYNTHESIS_AND_TRANSFER.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimgen
```

---

## The `layout.toml` Specification

```toml
[disk]
size = "2G"             # Supports B, K, M, G, T, or auto
table = "gpt"           # Partition table: 'gpt' or 'mbr'
alignment = "1M"        # Default partition alignment: 1 MiB

[[partitions]]
name = "ESP"
fs = "fat32"            # fat12, fat16, fat32, exfat, ext2, ext3, ext4, ntfs, iso9660
size = "256M"
bootable = true

[partitions.files]
"/EFI/BOOT/BOOTX64.EFI" = "target/x86_64-unknown-uefi/release/boot.efi"
"/loader/entries.conf"  = "config/entries.conf"

[[partitions]]
name = "rootfs"
fs = "ext4"
size = "auto"           # Automatically calculated from injected payloads + overhead

[partitions.files]
"/" = "build/rootfs_tree/"
```

---

## Usage Example

```rust
use rimgen::{build_config_on_io, LayoutConfig};
use rimio::prelude::MemRimIO;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load declarative layout configuration
    let layout = LayoutConfig::from_file(Path::new("layout.toml"))?;
    
    // 2. Compute total disk capacity
    let raw_len = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(&layout) * 512;
    let mut buffer = vec![0u8; raw_len as usize];
    let mut disk_io = MemRimIO::new(&mut buffer);

    // 3. Synthesize the complete partitioned disk image
    let report = build_config_on_io(&layout, &mut disk_io)?;
    println!(
        "Synthesized {} bytes across {} partitions in {:?}",
        report.total_bytes,
        report.partition_reports.len(),
        report.total_duration
    );

    Ok(())
}
```

---

## Related Documentation

- **[Storage Synthesis & Transfer Engine](../docs/SYNTHESIS_AND_TRANSFER.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
