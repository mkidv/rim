# rimfs-core

`rimfs-core` provides the shared foundational traits, common error types, and core abstractions for the RIM filesystem ecosystem.

## Architecture

This crate serves as the internal building block for filesystem implementations (`rimfs-fat`, `rimfs-exfat`, `rimfs-ext`, and `rimfs-ntfs`).

Key components:
- **Core Traits**: `FsFormatter`, `FsAllocator`, `FsInjector`, `FsResolver`, `FsChecker`, `FsNode`, `FsAttr`.
- **Error Handling**: Standard filesystem error kinds and convenience macros (`bail!`, `ensure!`).
- **Resolver**: `StdResolver` for walking host directories and resolving file trees in standard environments.
- **Checker Framework**: `ReachabilityTracker`, block/cluster leak detectors, and diagnostic types.
- **Common Utilities**: Volume helpers, path normalization, bitmask/bitmap manipulation, time conversions, and checksums.

## Usage

For typical applications, prefer using the unified [`rimfs`](../rimfs) facade crate rather than depending directly on `rimfs-core`.

```toml
[dependencies]
rimfs-core = { version = "0.6.0", default-features = false }
```

## Cargo Features

- `std`: Enables standard library integration (including `StdResolver`).
- `alloc`: Enables dynamic allocation support in `no_std` environments.
- `mem`: Enables in-memory buffer utilities.
- `uefi`: Enables UEFI firmware environment abstractions.

