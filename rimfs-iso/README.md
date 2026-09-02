# rimfs-iso

`rimfs-iso` is the pure-Rust ISO 9660 optical disk and hybrid image filesystem driver of the RIM ecosystem, supporting Joliet (Unicode UTF-16), Rock Ridge (POSIX permissions, symlinks), and El Torito (BIOS & UEFI booting with synthesized FAT EFI boot images).

## Features

- **Standard ISO 9660 & High Sierra Compliant**: 2048-byte sector geometry with ISO 733 and 723 both-endian integers.
- **Pre-Computed Planning Architecture**: Calculates all LBA assignments and descriptor layouts prior to sequential serialization, ensuring zero backtracking and optimal performance under WebAssembly and UEFI environments.
- **Joliet Extension**: Unicode UTF-16 Big-Endian filenames and directory hierarchies for Windows compatibility.
- **Rock Ridge (SUSP) Extension**: Full POSIX support for Unix file modes (`PX`), alternate long names (`NM`), symbolic links (`SL`), and UIDs/GIDs.
- **El Torito Boot Specification**:
  - BIOS No-Emulation bootloader support.
  - **UEFI Boot Support**: Automatic in-memory synthesis of FAT12/16/32 EFI System Partition boot images (containing `/EFI/BOOT/BOOTX64.EFI`) using `rimfs-fat`.
- **Pure `no_std + alloc` & Zero Subprocesses**: Creates bootable ISO images in-memory without `mkisofs`, `xorriso`, or root permissions.

## Usage

```toml
[dependencies]
rimfs-iso = { version = "0.8.2", default-features = false, features = ["alloc"] }
```

```rust
use rimfs_iso::prelude::*;
use rimio::MemRimIO;

let mut buf = vec![0u8; 100 * ISO_SECTOR_SIZE];
let mut io = MemRimIO::new(&mut buf);
let mut meta = IsoMeta::new(io.len(), Some("MY_LINUX_ISO")).unwrap();
meta.boot_efi = Some(bootx64_efi_bytes); // Automatic El Torito EFI boot synthesis!

let mut injector = IsoInjector::new(&mut io, &meta)?;
let mut tree = FsNode::new_dir("/");
injector.inject_tree(&mut tree)?;
```

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
