# rimgen

**rimgen** is a CLI tool and library for **declarative disk image generation**. It automates the complex pipeline of partitioning, formatting, and file injection into a single, reproducible workflow.

It is particularly useful for:
*   **CI/CD pipelines**: Generating fresh method disk images for testing.
*   **OS Development**: Creating bootable installation media.
*   **Embedded Systems**: Flashing images with pre-configured file hierarchies.

## Architecture

`rimgen` works by defining a `Layout` which is then processed by the `ImageBuilder`.

### 1. Layout Definition
Structures in `rimgen::layout` define the intent:
*   `DiskLayout`: The top-level image description (size, generic settings).
*   `Partition`: Defines size, type (EFI, Data), and the `content`.
*   `Content`: Specifiers for what goes inside a partition (Filesystem type, local source directory).

### 2. Output Engine
Reflected in `rimgen::out`, the engine executes the layout:
*   **Generates** a raw file or writes to a block device.
*   **Partitions** it using `rimpart`.
*   **Formats** partitions using `rimfs` (pure Rust implementation).
*   **Injects** files recursively using `rimfs` from the host machine to the target filesystem.

## Host Scripts Integration

For scenarios needing native OS utilities (e.g., using `diskpart`, `mkfs.ext4`, or mounting via loopback), `rimgen` includes a **Host Scripting** module (`feature = "host-scripts"`).

*   **Windows**: Generates and executes `diskpart` scripts to mount VHDs.
*   **Linux**: Uses `losetup`, `mkfs`, and `mount` for native handling.
*   **macOS**: Uses `hdiutil` and `diskutil`.

This allows `rimgen` to act as a cross-platform wrapper around OS-native tools when the pure-Rust implementation is unimplemented, not desired or insufficient.

## Supported Output Formats

| Format | Extension | Description |
|--------|-----------|-------------|
| Raw Image | `.img` | Raw disk image, directly writable to block devices |
| VHD | `.vhd` | Microsoft Virtual Hard Disk (fixed), for Hyper-V |
| VMDK | `.vmdk` | VMware Virtual Machine Disk (monolithic flat) |
| QCOW2 | `.qcow2` | QEMU Copy-On-Write v2 (flat, no compression) |
| VDI | `.vdi` | VirtualBox Disk Image (fixed) |

The format is automatically selected based on the output file extension.

## Configuration (`layout.toml`)

`rimgen` uses a TOML file to define the disk layout.

```toml
[disk]
alignment = "1M"         # Align partitions to 1MB boundaries (default: 1M)
guid = "12345678-1234-1234-1234-1234567890AB" # Optional Disk GUID

[[partitions]]
name = "EFI"
size = "128M"
type = "efi"
fs = "fat32"
mountpoint = "boot"      # Source directory: ./boot
label = "EFI_SYSTEM"     # Filesystem Label
uuid = "1234-ABCD"       # Filesystem UUID (Serial for FAT32)

[[partitions]]
name = "System"
size = "auto"            # Auto-calculate size based on content
type = "linux"
fs = "ext4"
mountpoint = "rootfs"    # Source directory: ./rootfs
label = "nixos"
uuid = "79ad9326-6663-417c-a0e2-2ed313264639"
bootable = true

[[partitions]]
name = "U-Boot"
size = "4M"
fs = "raw"               # Raw data partition
payload = "u-boot.bin"   # Binary file to write directly (mutually exclusive with mountpoint)
```

### Partition Configuration

| Field | Description | Type |
|-------|-------------|------|
| `name` | Partition name (GPT) | String |
| `size` | Size (`"512M"`, `"1G"`, or `"auto"`) | String |
| `type` | Partition Type GUID (e.g., `efi`, `linux`, `data`) | String |
| `fs` | Filesystem (`fat32`, `exfat`, `ext4`, `raw`) | String |
| `mountpoint` | Directory containing files to inject (relative to TOML) | String (Path) |
| `payload` | Binary file for `raw` partitions (byte-level copy) | String (Path) |
| `label` | Filesystem Label (e.g., volume name) | String |
| `uuid` | Filesystem UUID/Serial (hex string or UUID format) | String |
| `bootable` | Sets the Legacy BIOS Bootable flag | Boolean |

### Disk Configuration (`[disk]`)

| Field | Description | Default |
|-------|-------------|---------|
| `alignment` | Partition alignment (`"4K"`, `"1M"`) | `"1M"` |
| `guid` | Disk GUID (UUID format) | Random |

## Usage (CLI)

```bash
rimgen layout.toml -o image.img
```
