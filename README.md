# RIM (Rust Image Maker)

**RIM** is a modular, pure-Rust toolkit and engine for **generating, copying, manipulating, converting, verifying, and analyzing disk images and filesystems**.

Designed from the ground up for high reliability, streaming I/O, rootless userspace execution, cross-platform portability (Linux, macOS, Windows), and resource-constrained environments (`no_std`, `alloc`, and **UEFI firmware**).

[![CI](https://github.com/mkidv/rim/actions/workflows/ci.yml/badge.svg)](https://github.com/mkidv/rim/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

---

## 📦 Modular Ecosystem (15 Crates)

The project is structured into modular, decoupled crates:

| Crate | Role & Capabilities |
|---|---|
| **[`rimcli`](rimcli)** | Unified modern CLI producing the **`rim`** binary (`generate`, `convert`, `check`, `partition`, `inspect`, `copy`). |
| **[`rimgen`](rimgen)** | Pure library-first declarative storage synthesis engine (`Layout`, `LayoutConfig`, `build_on_io`). |
| **[`rimimg`](rimimg)** | `no_std`-capable virtual machine disk containers (**RAW**, **VHD**, **VMDK**, **QCOW2**, **VDI**), detection, logical image I/O adapters, wrap/unwrap, and conversions. |
| **[`rimhost`](rimhost)** | OS-native tooling integration (Windows PowerShell/Storage, Linux `losetup`/`mkfs`, macOS `diskutil`). |
| **[`rimfs`](rimfs)** | Unified public facade crate for filesystems. |
| **[`rimfs-core`](rimfs-core)** | Core traits (`FsFormatter`, `FsAllocator`, `FsInjector`, `FsResolver`, `FsChecker`), bitmaps, and resolvers. |
| **[`rimfs-fat`](rimfs-fat)** | FAT12, FAT16, FAT32 implementation, plus the 64-bit optimized **RimFAT** integrity extension. |
| **[`rimfs-exfat`](rimfs-exfat)** | ExFAT implementation with allocation bitmap tracking, upcase table compilation, and consistency checker. |
| **[`rimfs-ext`](rimfs-ext)** | Ext2, Ext3, and Ext4 with 48-bit physical extent trees, indirect block maps, 32-bit UID/GID, and fast/slow symlinks. |
| **[`rimfs-ntfs`](rimfs-ntfs)** | Pure-Rust NTFS 3.1 ($Boot, $MFT, non-resident $UpCase, $Secure, B-tree directory indexing, data runs). |
| **[`rimfs-tar`](rimfs-tar)** | POSIX UStar archive filesystem driver (`no_std + alloc`) with full injector/resolver/checker support. |
| **[`rimfs-zip`](rimfs-zip)** | ZIP archive filesystem driver (`no_std + alloc`) with Central Directory, ZIP64, and POSIX metadata. |
| **[`rimfs-iso`](rimfs-iso)** | ISO 9660 optical and hybrid disk driver with Joliet (Unicode), Rock Ridge (POSIX), and El Torito (UEFI/BIOS). |
| **[`rimpart`](rimpart)** | Partition table management for GPT, streaming GPT (`gpt_stream` on-the-fly CRC32), and MBR. |
| **[`rimio`](rimio)** | Low-level I/O abstraction (`StdRimIO`, `FileRimIO` positioned I/O, `MemRimIO`, `MmapRimIO`, `UefiRimIO`). |
| **[`sector-analyzer`](sector-analyzer)** | Forensic analysis tool (signatures, entropy, MFT/$Secure dumping, sector diffing). |

---

## ⚡ Key Features

- **Rootless & Zero-Dependency**:
  - Operates 100% in userspace: no `sudo`, no `losetup`, no kernel `mount`, no external C toolchains (`e2fsprogs`, `ntfs-3g`, `mkisofs`, `xorriso`).
  - Completely safe to run inside non-privileged Docker containers and CI/CD pipelines (GitHub Actions, GitLab CI).
- **Universal Portability**:
  - Identical behavior and deterministic output on **Linux**, **macOS** (Apple Silicon & Intel), and **Windows**.
- **Universal Logical Filesystem Copy (`rim copy`)**:
  - Direct logical transfers between host filesystems and disk image partitions (or between two disk images) without kernel mounting or root privileges.
  - In-memory `--dry-run` simulation using sparse copy-on-write page overlays (`OverlayRimIO`) with zero destination disk writes.
  - Destination-aware case-collision detection, metadata preservation policies, and cross-platform error handling.
- **Supported Filesystems & Archives**:
  - **FAT**: FAT12, FAT16, FAT32, and RimFAT.
  - **ExFAT**: Full formatting, directory injection, and consistency verification.
  - **EXT**: Ext2, Ext3, Ext4 (48-bit extent trees, block group descriptors, POSIX permissions & symlinks).
  - **NTFS**: Pure-Rust NTFS 3.1 ($MFT records, non-resident $UpCase, `$Secure` security descriptors, INDX B-tree directories).
  - **TAR**: POSIX UStar streaming archive creation, injection, extraction, and validation.
  - **ZIP**: Streaming creation, Central Directory parsing, ZIP64, Store/Deflate, and POSIX Unix extensions.
  - **ISO 9660**: Optical and hybrid disk creation with Joliet (UTF-16), Rock Ridge (POSIX permissions & symlinks), and El Torito UEFI/BIOS booting.
- **Supported Disk & Container Formats**:
  - Raw images: `.img`, `.raw`
  - Microsoft VHD: `.vhd` (fixed VHD)
  - VMware VMDK: `.vmdk` (monolithicFlat)
  - QEMU QCOW2: `.qcow2` (v2/v3 header)
  - VirtualBox VDI: `.vdi` (fixed)
- **Bare-Metal & Embedded Ready**:
  - `no_std`, `alloc`, **WebAssembly** (`wasm32-unknown-unknown`), and native **UEFI** firmware (`EFI_BLOCK_IO_PROTOCOL`) support.

---

## 🚀 Installation

Install the **`rim`** CLI from source or via Cargo:

```bash
cargo install --path rimcli
```

---

## 🛠️ CLI Usage (`rim`)

The `rim` command-line tool provides 6 core subcommands:

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

### 6. Logical Filesystem Copy (`copy`)

Perform high-performance, logical transfers between host filesystems and disk image partitions (or between two disk images) without mounting or requiring root privileges:

```bash
# Inject a host folder into a FAT32 partition within a disk image
rim copy ./efi_payload/ disk.img:1:/EFI/BOOT/

# Extract a directory from an EXT4 partition to the host
rim copy disk.img:2:/etc/ ./extracted_etc/

# Transfer logically between partitions or disk images (e.g. FAT to EXT4)
rim copy disk1.img:1:/data disk2.img:2:/backup

# Dry-run simulation (purely in-memory verification without touching disk)
rim copy ./dist/ disk.img:1:/ --dry-run

# Overwrite policy and metadata handling
rim copy ./source disk.img:1:/ --overwrite replace --metadata preserve --unsupported warn
```

---

## 💻 Library Usage (`rimgen`)

`rimgen` can be embedded directly into any Rust application to synthesize storage layouts in memory or over custom `RimIO` streams without writing temporary files to disk:

```rust
use rimgen::{build_config_on_io, LayoutConfig};
use rimio::prelude::MemRimIO;

let layout = LayoutConfig::from_file(std::path::Path::new("layout.toml"))?;
let raw_len = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(&layout) * 512;

let mut buffer = vec![0u8; raw_len as usize];
let mut memory_disk = MemRimIO::new(&mut buffer);
let report = build_config_on_io(&layout, &mut memory_disk)?;

println!("Synthesized {} bytes in {:?}", report.total_bytes, report.total_duration);
```

---

## Support Matrix

| Format | Format | Inject | Resolve/read | Fast check | Deep check | `no_std + alloc` | Known limitations |
|---|---:|---:|---:|---:|---:|---:|---|
| FAT12/16/32 | Yes | Yes | Yes | Yes | Yes | Yes | FAT32 file sizes are bounded by the on-disk 32-bit file size field. |
| RimFAT | Yes | Yes | Yes | Yes | Yes | Yes | RIM integrity extensions are RIM-specific and not a portable FAT extension. |
| exFAT | Yes | Yes | Yes | Yes | Yes | Yes | Advanced vendor extensions are outside the current scope. |
| EXT2/3/4 | Yes | Yes | Yes | Yes | Yes | Yes | Focuses on generated images, common extents/block maps, POSIX metadata and symlinks; not a complete kernel-grade EXT implementation. |
| NTFS 3.1 | Yes | Yes | Yes | Yes | Yes | Yes | Supports generated NTFS images with resident/non-resident data, runlists, indexes and core system files; not every Windows NTFS feature is implemented. |
| TAR UStar | Yes | Yes | Yes | Yes | Yes | Yes | POSIX UStar-oriented; non-UStar vendor extensions are limited. |
| ZIP | Yes | Yes | Yes | Yes | Yes | Yes | Store/Deflate plumbing and ZIP64 structures are present; encrypted archives are not supported. |
| ISO 9660 | Yes | Yes | Yes | Yes | Yes | Yes | Joliet/Rock Ridge/El Torito coverage targets generated images, not arbitrary mastering edge cases. |

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
