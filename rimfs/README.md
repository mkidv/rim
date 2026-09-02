# rimfs

`rimfs` is the unified public facade crate for the RIM filesystem ecosystem.

It provides an ergonomic, feature-gated API while implementations are cleanly separated into specialized internal crates:
- `rimfs-core`: Shared core traits, errors, and abstractions.
- `rimfs-fat`: FAT12, FAT16, FAT32, and RimFAT implementations.
- `rimfs-exfat`: ExFAT implementation.
- `rimfs-ext`: Ext2, Ext3, and Ext4 implementations.
- `rimfs-ntfs`: Pure-Rust NTFS 3.1 implementation.
- `rimfs-tar`: POSIX UStar TAR archive implementation.
- `rimfs-zip`: Streaming ZIP archive implementation with ZIP64 & POSIX extensions.
- `rimfs-iso`: ISO 9660 / Joliet / Rock Ridge / El Torito optical & hybrid disk implementation.

## Installation

```toml
[dependencies]
rimfs = "0.8.2"
```

## Cargo Features

- `fat`: Enables FAT12/16/32 and RimFAT support.
- `exfat`: Enables ExFAT support.
- `ext`: Enables EXT (Ext2/3/4) support.
- `ntfs`: Enables NTFS 3.1 support.
- `tar`: Enables TAR archive support.
- `zip`: Enables ZIP archive support.
- `iso`: Enables ISO 9660 / Joliet / Rock Ridge / El Torito support.
- `std`: Enables standard library integration (`StdResolver`, File I/O).
- `alloc`: Enables dynamic allocation support in `no_std` environments.
- `uefi`: Enables UEFI pre-boot environment integration.

## Architecture

`rimfs` re-exports:
- Shared abstractions from `rimfs-core`.
- Specialized filesystem modules based on enabled features (`rimfs::fat`, `rimfs::exfat`, `rimfs::ext`, `rimfs::ntfs`, `rimfs::tar`, `rimfs::zip`, `rimfs::iso`).

Each filesystem follows a standard composable pipeline:
- `Formatter`: Low-level volume structure initialization.
- `Allocator`: Cluster / block allocation tracking.
- `Injector`: Streaming file and directory injection.
- `Resolver`: Dynamic path resolution from host sources.
- `Checker`: Filesystem metadata consistency verification.

## Usage

```rust
use rimfs::ext::{ExtAllocator, ExtFormatter, ExtInjector, ExtMeta};
use rimfs::core::traits::FsNode;
use rimio::{MemRimIO, StdRimIO};
use std::fs::File;

let mut file = File::options().read(true).write(true).open("disk.img")?;
let mut disk = StdRimIO::new(&mut file);
let meta = ExtMeta::new(disk.len(), Some("MY_VOLUME"));

ExtFormatter::new(&mut disk, &meta).format()?;

let mut allocator = ExtAllocator::new(&meta);
let mut injector = ExtInjector::new(&mut disk, &mut allocator, &meta);
injector.set_root_context(&FsNode::new_dir("/"))?;

let mut data = b"hello".to_vec();
let mut src = MemRimIO::new(&mut data);
injector.write_file("hello.txt", &mut src, 5, &Default::default())?;
injector.flush()?;
```

## Notes

- Benchmarks are maintained within this facade crate.
- See `RIMFAT_SPEC.md` for the technical specification of the RimFAT 64-bit extension.

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
