# rimfs

**rimfs** is a comprehensive filesystem manipulation library written in Rust, designed for **high-reliability embedded and system programming**. It provides `no_std` compatible, zero-allocation (where possible) implementations of standard filesystems.

Unlike typical filesystem drivers that mount a drive for OS usage, `rimfs` is specialized for **filesystem generation, injection, and off-line verification**. It allows you to create formatted disk images, inject files into them, and verify their integrity without root privileges or kernel drivers.

## Installation & Features

Add `rimfs` to your `Cargo.toml`:

```toml
[dependencies]
rimfs = { version = "0.5.0", default-features = false, features = ["std", "fat32"] }
```

### Features

By default, all filesystems (`fat32`, `exfat`, `ext4`) and `std` support are enabled. You can optimize compilation time and binary size by disabling default features and selecting only what you need:

- **Filesystems**:
  - `fat32`: Enables FAT32 support.
  - `exfat`: Enables ExFAT support.
  - `ext4`: Enables EXT4 support.

- **System**:
  - `std`: Enables standard library support (File I/O, System Time).
  - `alloc`: Enables `alloc` crate support (required for `no_std` if `std` is disabled).
  - `uefi`: Enables UEFI specific optimizations and bindings.


## Supported Filesystems

### 🐧 EXT4 (Extended Filesystem 4)
A robust implementation of the Linux standard filesystem.
*   **Modules**: `allocator`, `checker`, `formatter`, `injector`, `resolver`.
*   **Features**:
    *   **Advanced Formatting**: Supports `Flex Block Groups` and `Sparse Superblocks`.
    *   **Extent Injection**: Writes files using efficient extent trees.
    *   **Deep consistency checking**: Validates inodes, bitmaps, and directory connectivity.
    *   **State**: Alpha (Read/Write/Check fully functional for basic images).

### ⚡ ExFAT (Extensible File Allocation Table)
Optimized for high-capacity removable storage.
*   **Modules**: `allocator`, `checker`, `formatter`, `injector`, `resolver`, `upcase`.
*   **Features**:
    *   **Bitmap Allocation**: Fast cluster allocation using the Allocation Bitmap.
    *   **Upcase Table**: Full unicode upper-casing support for filename compatibility.
    *   **Large File Support**: Handles files >4GB natively.
    *   **State**: Beta (Stable Read/Write).

### 💾 FAT32 (File Allocation Table)
The legacy standard for broad compatibility.
*   **Modules**: `allocator`, `checker`, `formatter`, `injector`, `resolver`.
*   **Features**:
    *   **LFN Support**: Long File Names for modern paths.
    *   **Cross-Platform**: Generates images compatible with Windows, Linux, and macOS.
    *   **State**: Stable.

## Architecture

`rimfs` is built on a modular "Injector/Resolver" architecture:

*   **Formatter**: Initializes the filesystem structures (Superblocks, FATs, Bitmaps).
*   **Allocator**: Manages free space (bitmaps, FAT chains) in memory for the Injector.
*   **Injector**: "Injects" files and folders. uses a **Context Stack** to traverse directories and stream data from any `RimIO` source.
*   **Resolver**: Traverses the filesystem to find files and directories (Read-only access).
*   **Checker**: Performs `fsck`-like validation of the structures.

## Verification & Performance

`rimfs` functionality is strictly validated:
*   **Integration Tests**: Found in `examples/`, validating the full "format-inject-check" cycle for every filesystem.
*   **Checkers**: Each filesystem implements a `Checker` module that verifies the consistency of the generated image (bitmaps vs inodes, connectivity).
*   **Benchmarks**: Latency and throughput are measured (via `criterion`) in `benches/`.

## Usage

The process typically follows a 3-step pipeline: **Format -> Allocate -> Inject**.

```rust
use rimfs::ext4::{Ext4Meta, Ext4Formatter, Ext4Allocator, Ext4Injector};
use rimfs::core::traits::{FsNodeInjector, FsNode};
use rimfs::core::FileAttributes;
use rimio::{StdRimIO, MemRimIO, RimIO};
use std::fs::File;

// 1. Open a raw disk image
let mut file = File::options().read(true).write(true).open("disk.img")?;
let mut disk = StdRimIO::new(&mut file);
let meta = Ext4Meta::new(disk.len(), Some("MY_VOLUME"));

// 2. Format
Ext4Formatter::new(&mut disk, &meta).format()?;

// 3. Inject
let mut allocator = Ext4Allocator::new(&meta);
let mut injector = Ext4Injector::new(&mut disk, &mut allocator, &meta);

// Initialize Root Context (Critical step: loads root inode/cluster)
injector.set_root_context(&FsNode::new_dir("/"))?;

// Inject a file (streaming from a source IO)
// Assuming kernel_bytes is a &[u8] or Vec<u8>
let mut kernel_data = MemRimIO::new(&mut kernel_bytes);
let attr = FileAttributes::default(); // default attributes for a file

injector.write_file("kernel.bin", &mut kernel_data, kernel_bytes.len() as u64, &attr)?;
injector.flush()?; // Commit changes to disk (Superblock, BGDT, etc.)
```
