# rimpart

**rimpart** is a pure Rust library for manipulating disk partition tables, compliant with **UEFI/GPT** standards and legacy **MBR** layouts. It supports `no_std` environments, making it suitable for firmware and embedded bootloaders.

## Core Modules

### 🧭 `gpt` (Standard GPT)
Full implementation of the GUID Partition Table.
*   **Structs**: `Gpt`, `GptHeader`, `GptEntry`.
*   **Features**:
    *   **CRC32 Validation**: Ensures integrity of header and partition array.
    *   **Dual-Header Management**: Handles Primary and Backup headers automatically.
    *   **LBA Addressing**: Precise 64-bit LBA (Logical Block Address) manipulation.

### 🌊 `gpt_stream` (Streaming GPT)
Designed for memory-constrained environments where loading the full partition table is impossible.
*   **API**: Iterates over partition entries one by one directly from the `RimIO` source.
*   **Use-case**: Bootloaders scanning for a kernel partition on a massive disk.

### 💾 `mbr` (Master Boot Record)
Legacy support and protection.
*   **Protective MBR**: Generates the standard protective MBR required by the UEFI spec to prevent legacy tools from corrupting GPT disks.
*   **Legacy Parsing**: Read basic primary partitions (CHS/LBA).

### 🔍 `scanner` (Alloc only)
High-level utilities to discover partitions.
*   `scan_disk`: Automatically finds GPT or MBR and returns a list of partitions.
*   `detect_partition_offset_by_type_guid`: Locates specific partitions (e.g., EFI System Partition) by their GUID.

## Usage

```rust
use rimpart::gpt::Gpt;
use rimpart::guids;
use rimio::prelude::*;
use std::fs::File;

let mut file = File::open("disk.img")?;
let mut disk = StdRimIO::new(&mut file);
let sector_count = disk.len() / 512;

// Create a new GPT with a Protective MBR
let mut gpt = Gpt::new(sector_count);

// Add an EFI System Partition (100MB)
gpt.add_partition(
    "EFI System",
    2048,               // Start LBA
    204800,             // Size in sectors
    guids::EFI_SYSTEM_PARTITION
);

// Write to disk
gpt.write(&mut disk)?;
```
