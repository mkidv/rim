# RIM (Rust Image Maker)

**RIM** is a modular, pure-Rust toolkit and engine for **generating, manipulating, converting, verifying, and analyzing disk images and filesystems**.

Designed from the ground up for high reliability, streaming I/O, rootless userspace execution, cross-platform portability (Linux, macOS, Windows), and resource-constrained environments (`no_std`, `alloc`, and **UEFI firmware**).

[![CI](https://github.com/mkidv/rim/actions/workflows/ci.yml/badge.svg)](https://github.com/mkidv/rim/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## 📦 Modular Ecosystem (12 Crates)

The project is structured into modular, decoupled crates:

| Crate | Role & Capabilities |
|---|---|
| **[`rimcli`](rimcli)** | Unified modern CLI producing the **`rim`** binary (`generate`, `convert`, `check`, `partition`, `inspect`). |
| **[`rimgen`](rimgen)** | Pure library-first declarative storage synthesis engine (`DiskLayout`, `ImageBuilder`, `build_on_io`). |
| **[`rimimg`](rimimg)** | Virtual machine disk containers (**RAW**, **VHD**, **VMDK**, **QCOW2**, **VDI**), detection, wrap/unwrap, and conversions. |
| **[`rimhost`](rimhost)** | OS-native tooling integration (Windows PowerShell/Storage, Linux `losetup`/`mkfs`, macOS `diskutil`). |
| **[`rimfs`](rimfs)** | Unified public facade crate for filesystems. |
| **[`rimfs-core`](rimfs-core)** | Core traits (`FsFormatter`, `FsAllocator`, `FsInjector`, `FsResolver`, `FsChecker`), bitmaps, and resolvers. |
| **[`rimfs-fat`](rimfs-fat)** | FAT12, FAT16, FAT32 implementation, plus the 64-bit optimized **RimFAT** integrity extension. |
| **[`rimfs-exfat`](rimfs-exfat)** | ExFAT implementation with allocation bitmap tracking, upcase table compilation, and consistency checker. |
| **[`rimfs-ext`](rimfs-ext)** | Ext2, Ext3, and Ext4 with 48-bit physical extent trees, indirect block maps, 32-bit UID/GID, and fast/slow symlinks. |
| **[`rimfs-ntfs`](rimfs-ntfs)** | Pure-Rust NTFS 3.1 ($Boot, $MFT, non-resident $UpCase, $Secure, B-tree directory indexing, data runs). |
| **[`rimpart`](rimpart)** | Partition table management for GPT, streaming GPT (`gpt_stream` on-the-fly CRC32), and MBR. |
| **[`rimio`](rimio)** | Low-level I/O abstraction (`StdRimIO`, `FileRimIO` positioned I/O, `MemRimIO`, `MmapRimIO`, `UefiRimIO`). |
| **[`sector-analyzer`](sector-analyzer)** | Forensic analysis tool (signatures, entropy, MFT/$Secure dumping, sector diffing). |

---

## ⚡ Key Features

- **Rootless & Zero-Dependency**:
  - Operates 100% in userspace: no `sudo`, no `losetup`, no kernel `mount`, no external C toolchains (`e2fsprogs`, `ntfs-3g`).
  - Completely safe to run inside non-privileged Docker containers and CI/CD pipelines (GitHub Actions, GitLab CI).
- **Universal Portability**:
  - Identical behavior and deterministic output on **Linux**, **macOS** (Apple Silicon & Intel), and **Windows**.
- **Supported Filesystems**:
  - **FAT**: FAT12, FAT16, FAT32, and RimFAT.
  - **ExFAT**: Full formatting, directory injection, and consistency verification.
  - **EXT**: Ext2, Ext3, Ext4 (48-bit extent trees, block group descriptors, POSIX permissions & symlinks).
  - **NTFS**: Pure-Rust NTFS 3.1 ($MFT records, non-resident $UpCase, `$Secure` security descriptors, INDX B-tree directories).
- **Supported Disk & Container Formats**:
  - Raw images: `.img`, `.raw`
  - Microsoft VHD: `.vhd` (fixed VHD)
  - VMware VMDK: `.vmdk` (monolithicFlat)
  - QEMU QCOW2: `.qcow2` (v2/v3 header)
  - VirtualBox VDI: `.vdi` (fixed)
- **Bare-Metal & Embedded Ready**:
  - `no_std`, `alloc`, and native **UEFI** firmware (`EFI_BLOCK_IO_PROTOCOL`) support.

---

## 🚀 Installation

Install the **`rim`** CLI from source or via Cargo:

```bash
cargo install --path rimcli
```

---

## 🛠️ CLI Usage (`rim`)

The `rim` command-line tool provides 5 core subcommands:

### 1. Generating a Disk Image (`generate` / `build`)

Declare your disk layout in a `layout.toml` file:

```toml
[disk]
alignment = "1M"

[[partitions]]
name = "ESP"
fs = "fat32"
size = "64M"
bootable = true
type = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" # EFI System Partition GUID
mountpoint = "efi/*" # Local folder to inject

[[partitions]]
name = "ROOTFS"
fs = "ext4"
size = "500M"
label = "ROOTFS"
mountpoint = "rootfs/*"
```

Generate the disk image (format is auto-detected from output extension):

```bash
# Generate raw disk image
rim generate layout.toml --output disk.img

# Generate VM disk containers directly (VHD, QCOW2, VMDK, VDI)
rim generate layout.toml --output disk.vhd
rim generate layout.toml --output disk.qcow2
```

### 2. Inspecting Disks & Containers (`inspect`)

Deeply inspect container formats, partition tables, and detected filesystems with colorized status badges:

```bash
rim inspect disk.qcow2
```

### 3. Converting Between Container Formats (`convert`)

Convert directly between any supported disk image container formats with live ETA and transfer speed reporting:

```bash
rim convert disk.img disk.vhd
rim convert disk.img disk.qcow2
rim convert disk.vhd disk.vdi
```

### 4. Partition Scheme Inspector (`partition`)

Display partition LBA offsets, sizes, kinds, and GUIDs:

```bash
rim partition disk.img
```

### 5. Filesystem Integrity Checker (`check`)

Verify GPT structures and filesystem consistency offline:

```bash
rim check disk.img
```

---

## 💻 Library Usage (`rimgen`)

`rimgen` can be embedded directly into any Rust application to synthesize storage layouts in memory or over custom `RimIO` streams without writing temporary files to disk:

```rust
use rimgen::{DiskLayout, ImageBuilder, Partition, Size, Filesystem};
use rimio::prelude::MemRimIO;

// 1. Define layout in code or parse from TOML
let layout = DiskLayout::from_file(std::path::Path::new("layout.toml"))?;

// 2. Build directly onto an in-memory buffer or block stream
let mut memory_disk = MemRimIO::new();
let report = rimgen::build_on_io(&layout, &mut memory_disk)?;

println!("Synthesized {} bytes in {:?}", report.total_bytes, report.total_duration);
```

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
