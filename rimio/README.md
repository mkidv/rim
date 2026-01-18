# rimio

**rimio** is the foundational I/O layer for the RIM ecosystem. It provides a standardized, zero-cost abstraction for block-based devices, enabling filesystem drivers to run identically on host operating systems, embedded hardware, and UEFI firmware.

It is heavily inspired by `std::io` but specialized for non-stream (random-access, block-aligned) media.

## Core API

### `RimIO` Trait
The interface that all storage backends must implement.
```rust
pub trait RimIO {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult;
    fn write_at(&mut self, offset: u64, buf: &[u8]) -> RimIOResult;
    fn flush(&mut self) -> RimIOResult;
    fn len(&self) -> u64;
}
```

### Extensions
`rimio` provides powerful extension traits automatically implemented for any `RimIO` type:

*   **`RimIOExt`**: High-level helpers.
    *   `read_in_chunks`, `write_in_chunks`: Break down large IO into safe buffer sizes.
    *   `read_multi_at`: Optimized scatter/gather reads (coalesces adjacent requests).
    *   `write_primitive`: Endian-aware integer writes.
*   **`RimIOStreamExt`**: Streaming capabilities.
    *   `read_chunks_streamed`: Process large datasets (like FAT tables) via callbacks, keeping memory usage constant.
*   **`RimIOStructExt`**: Type-safe I/O.
    *   `read_struct::<T>` / `write_struct`: Read/Write `zerocopy` structs directly from disk.

## Statistics & Tracing

`rimio` includes built-in tools for performance analysis:

*   **`IOCounter`**: A transparent wrapper that tracks:
    *   Total read/write/flush counts and bytes.
    *   **Alignment metrics**: Distinguishes between aligned (fast) and unaligned (slow) operations.
    *   Max operation sizes.
*   **`IoStats`**: A `no_std` friendly struct to store and display these metrics.

```rust
use rimio::prelude::*;
use std::fs::File;

let mut file = File::open("disk.img")?;
let mut monitored_disk = IOCounter::new(StdRimIO::new(&mut file));

// Perform operations...
monitored_disk.read_at(0, &mut buf)?;

println!("{}", monitored_disk.stats);
// Output: Reads: 1 ops | total 512 B | aligned 100%
```

## Backends

*   **`std::StdRimIO`**: Wraps `std::io` compatible types (File, TcpStream, etc.). Supports seeking and resizing if the underlying type does.
*   **`mem::MemRimIO`**: Wraps a `&mut [u8]` or `Vec<u8>`. Perfect for testing or RAM disks.
*   **`uefi::UefiRimIO`** (Feature `uefi`): Wraps the UEFI `RimIO` protocol for bootloader development.

## Features

*   **`std`** (default): Enables file-system backends.
*   **`alloc`**: Enables heap-dependent optimizations (larger buffers for copy operations).
*   **`mem`**: Enables in-memory backends.
*   **`uefi`**: Enables UEFI specific protocols.
