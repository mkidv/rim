# rimio

[![crates.io](https://img.shields.io/crates/v/rimio.svg)](https://crates.io/crates/rimio)
[![Documentation](https://docs.rs/rimio/badge.svg)](https://docs.rs/rimio)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![no_std](https://img.shields.io/badge/no__std-supported-green.svg)](#features)

**`rimio`** is the foundational zero-allocation, positioned random-access I/O layer of the **[RIM](../README.md)** ecosystem (Layer 0).

It provides decoupled, seek-free block storage abstractions that execute identically across standard operating systems, in-memory buffers, bare-metal UEFI firmware, and WebAssembly.

---

## Architecture & System Role

In traditional storage libraries, `Read` and `Write` rely on internal cursor state (`seek`), which causes race conditions and performance degradation in multi-partition or concurrent scenarios. `rimio` enforces **explicit offset positioning** for all operations, enabling:

- **Partition Sub-Slicing**: [`BoundedRimIO`](src/mem.rs) exposes sub-ranges $[O, O + L)$ as isolated, zero-indexed virtual block devices.
- **Copy-on-Write Simulation**: [`OverlayRimIO`](src/sparse.rs) and `PagedOverlayRimIO` intercept writes into an ephemeral in-memory sparse page table, powering `rim copy --dry-run` with zero physical disk writes.
- **Zero-Allocation Operation**: In-memory slices (`SliceRimIO`) and buffers (`MemRimIO`) run with zero heap allocations in `#![no_std]` environments.

For deeper architectural details, see **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimio
```

For constrained or embedded `no_std` environments (alloc-only):

```bash
cargo add rimio --no-default-features --features alloc
```

---

## Core Traits

### `RimRead` & `RimWrite`

```rust
pub trait RimRead {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult;
    fn total_size(&mut self) -> RimIOResult<u64> {
        Err(RimIOError::Unsupported)
    }
}

pub trait RimWrite {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> RimIOResult;
    fn zero_at(&mut self, offset: u64, len: u64) -> RimIOResult;
    fn flush(&mut self) -> RimIOResult;
}
```

### `RimIO`

The unified read/write block device abstraction with partition offset tracking:

```rust
pub trait RimIO: RimRead + RimWrite {
    fn set_offset(&mut self, partition_offset: u64) -> u64;
    fn partition_offset(&self) -> u64;
}
```

---

## Usage Example

```rust
use rimio::prelude::*;

fn main() -> RimIOResult<()> {
    // 1. In-memory block device backed by a byte buffer
    let mut buffer = vec![0u8; 1024 * 1024]; // 1 MiB
    let mem_disk = MemRimIO::new(&mut buffer);

    // 2. Wrap with IOCounter for real-time throughput & alignment profiling
    let mut monitored_disk = IOCounter::new(mem_disk);

    // 3. Perform positioned write and read operations
    monitored_disk.write_at(0, b"RIM storage engine payload")?;
    monitored_disk.flush()?;

    let mut read_buf = [0u8; 26];
    monitored_disk.read_at(0, &mut read_buf)?;
    assert_eq!(&read_buf, b"RIM storage engine payload");

    // 4. Query total size and alignment statistics
    println!("Total device size: {} bytes", monitored_disk.total_size()?);
    println!("I/O Profile:\n{}", monitored_disk.stats);
    // Output: Reads: 1 ops | total 26 B | aligned 100%

    Ok(())
}
```

---

## Storage Backends

| Backend | Feature | Description |
|---|---|---|
| **`MemRimIO`** | Default / `mem` | In-memory mutable byte slice (`&mut [u8]`), `no_std` friendly. |
| **`SliceRimIO`** | Default / `mem` | Read-only in-memory slice (`&[u8]`) with zero heap allocation. |
| **`VecRimIO`** | `alloc` | Heap-allocated owned vector implementation. |
| **`FileRimIO`** | `std` | Positioned host file I/O (`pread`/`pwrite` on Unix, `SeekRead`/`SeekWrite` on Windows). |
| **`StdRimIO`** | `std` | Adapts any standard `Read + Write + Seek` type to `RimIO`. |
| **`MmapRimIO`** | `mmap` | High-throughput memory-mapped file backend. |
| **`OverlayRimIO`** | `alloc` | Copy-on-Write sparse page overlay for zero-write simulations. |
| **`UefiRimIO`** | `uefi` | Direct integration with UEFI firmware `EFI_BLOCK_IO_PROTOCOL`. |

---

## Cargo Features

- **`std`** (default): Enables host OS file backends (`FileRimIO`, `StdRimIO`).
- **`mmap`**: Enables memory-mapped I/O support via `MmapRimIO`.
- **`alloc`**: Enables heap-dependent features (sparse overlays, `VecRimIO`, `Box<dyn RimIO>`).
- **`mem`**: Enables in-memory slice and buffer abstractions.
- **`uefi`**: Enables bare-metal UEFI block protocol bindings.

---

## Related Documentation

- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)**
- **[Workspace Overview](../README.md)**
