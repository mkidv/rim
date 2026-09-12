# rimpart

[![crates.io](https://img.shields.io/crates/v/rimpart.svg)](https://crates.io/crates/rimpart)
[![Documentation](https://docs.rs/rimpart/badge.svg)](https://docs.rs/rimpart)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#cargo-features)

**`rimpart`** is the partition table management engine of the **[RIM](../README.md)** ecosystem (Layer 1).

It provides pure-Rust, `no_std`-capable generation, validation, and scanning of **GUID Partition Tables (GPT)**, single-pass **Streaming GPT (`gpt_stream`)**, and legacy **Master Boot Records (MBR)**.

---

## Architecture & System Role

`rimpart` executes directly over [`RimIO`](../rimio) block streams without intermediate temporary files:

- **Full UEFI GPT Compliance**: Generates Primary GPT Headers (LBA 1), Partition Entry Arrays (LBA 2..33), and mirrors them accurately to Backup GPT structures at the physical end of the volume with CRC32 integrity checks.
- **Streaming GPT (`gpt_stream`)**: Single-pass partition generator computing CRC32 on-the-fly, enabling partition generation on non-seekable streams and constrained embedded targets without buffering the disk.
- **Protective & Legacy MBR**: Synthesizes protective MBRs (`0xEE`) to shield GPT disks against legacy partitioning corruption, alongside standard MBR primary partition tables.
- **Partition Scanner**: Inspects unknown disks, detects partition schemes, and resolves sector ranges.

For detailed partition topologies and LBA layouts, see **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimpart
```

For constrained `no_std` environments:

```bash
cargo add rimpart --no-default-features --features alloc
```

---

## Usage Example

```rust
use rimio::prelude::MemRimIO;
use rimpart::gpt::{self, GptEntry};
use rimpart::guids;
use rimpart::mbr;
use rimpart::scanner::scan_disk;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sector_size = 512u64;
    let total_sectors = 20_000u64; // ~10 MiB disk
    let mut buffer = vec![0u8; (sector_size * total_sectors) as usize];
    let mut disk = MemRimIO::new(&mut buffer);

    // 1. Write standard Protective MBR at LBA 0
    mbr::write_mbr_protective(&mut disk, total_sectors)?;

    // 2. Define partition entries
    let esp = GptEntry::new(
        guids::GPT_PARTITION_TYPE_ESP,
        [0x01; 16], // Unique Partition GUID
        2048,       // Starting LBA (1 MiB alignment)
        4095,       // Ending LBA
        0,          // Flags
        "ESP",      // Partition name
    );

    let root = GptEntry::new(
        guids::GPT_PARTITION_TYPE_LINUX,
        [0x02; 16],
        4096,
        total_sectors - 34, // Last usable LBA before backup GPT
        0,
        "rootfs",
    );

    // 3. Write Primary & Backup GPT tables with automated CRC32 calculation
    let disk_guid = [0xAA; 16];
    gpt::write_gpt_from_entries(&mut disk, &[esp, root], total_sectors, disk_guid)?;

    // 4. Scan disk and inspect detected partitions
    let scan_result = scan_disk(&mut disk)?;
    println!("Detected Partitions:\n{}", scan_result);

    Ok(())
}
```

---

## Key Modules

- **[`gpt`](src/gpt.rs)**: Standard GPT header and entry structures, LBA alignment helpers (`align_lba_1m`), and CRC32 verification.
- **[`gpt_stream`](src/gpt_stream.rs)**: Streaming reader (`GptStreamReader`) and writer (`GptStreamWriter`) for sequential environments.
- **[`mbr`](src/mbr.rs)**: Protective MBR generation and legacy MBR sector parsing.
- **[`scanner`](src/scanner.rs)**: Auto-detection of partition schemes and partition offset resolution.
- **[`guids`](src/guids.rs)**: Standard partition type GUIDs (EFI System Partition, Linux Root, Microsoft Basic Data, etc.).

---

## Cargo Features

- **`std`** (default): Enables standard library formatting and error integrations.
- **`alloc`**: Enables heap-dependent partition vectors and disk scanning utilities.

---

## Related Documentation

- **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
