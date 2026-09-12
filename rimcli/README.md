# rimcli

[![crates.io](https://img.shields.io/crates/v/rimcli.svg)](https://crates.io/crates/rimcli)
[![Documentation](https://docs.rs/rimcli/badge.svg)](https://docs.rs/rimcli)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)
[![Live Demo](https://img.shields.io/badge/Live%20Demo-mki.dev%2Frim-ff4081?style=flat-square&logo=googlechrome&logoColor=white)](https://mki.dev/rim)

**`rimcli`** is the unified command-line application of the **[RIM](../README.md)** storage toolkit (Layer 6), producing the **`rim`** executable.

It exposes pure-Rust disk image generation, format conversion, filesystem verification, partition inspection, and rootless file transfer capabilities directly to developers and CI/CD pipelines.

> 🚀 **Try RIM live in your browser:** [mki.dev/rim](https://mki.dev/rim) — Zero-install interactive layout playground, client-side WebAssembly synthesis, and in-browser UEFI VM demo!

---

## Installation

Install the **`rim`** binary from source:

```bash
cargo install --path rimcli
```

Or install from crates.io:

```bash
cargo install rimcli
```

---

## Command Reference

### 1. `rim generate` / `rim build`
Synthesizes a bootable or raw disk image from a declarative `layout.toml` specification:

```bash
# Build disk defined in layout.toml (default output: disk.img or name in layout)
rim generate layout.toml

# Override target output file path
rim generate layout.toml --output custom_disk.qcow2
```

### 2. `rim copy` (Universal Rootless Transfer)
Direct logical transfers between host filesystems and disk image partitions (or between two disk images) without root privileges or kernel mounting. Partitions are addressed using **1-based index syntax** (`disk.img:<part>:<path>`):

```bash
# Copy file from host into partition 1 of a raw disk image
rim copy ./payload.bin disk.img:1:/boot/payload.bin

# Copy directory from partition 2 of a QCOW2 image to local host directory
rim copy disk.qcow2:2:/etc/nginx/ ./local_nginx/

# Transfer directly between two disk images with different filesystems (e.g. EXT4 -> NTFS)
rim copy linux.img:2:/data/file.db windows.vhd:1:/data/file.db

# In-memory dry-run simulation (zero disk writes, validates space & paths)
rim copy ./dist/ disk.img:1:/ --dry-run

# Advanced metadata and overwrite policies
rim copy disk.img:2:/etc/ ./extracted_etc/ \
  --overwrite replace \
  --metadata preserve-all \
  --unsupported warn
```

### 3. `rim convert`
Converts between virtual disk container formats (RAW, VHD, VMDK, QCOW2, VDI) in streaming mode with zero temporary intermediate files:

```bash
# Convert a raw image to a dynamically growing QCOW2 image
rim convert disk.img disk.qcow2

# Convert fixed VHD to VirtualBox VDI
rim convert image.vhd image.vdi
```

### 4. `rim inspect`
Deeply inspects an image file to identify container formats, partition schemes, and embedded filesystem signatures:

```bash
rim inspect disk.qcow2
```

### 5. `rim partition`
Displays partition table information (MBR or GPT), partition GUIDs, sector counts, and LBA alignment:

```bash
rim partition disk.img
```

### 6. `rim check`
Performs offline structural and reachability verification on filesystems inside the image:

```bash
rim check disk.img
```

---

## Related Documentation

- **[Storage Synthesis & Transfer Engine](../docs/SYNTHESIS_AND_TRANSFER.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Containers, Partitions & Storage I/O](../docs/CONTAINERS.md)**
- **[Filesystem Engines & Contracts](../docs/FILESYSTEMS.md)**
- **[Workspace Overview](../README.md)**
