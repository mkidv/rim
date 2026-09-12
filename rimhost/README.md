# rimhost

[![crates.io](https://img.shields.io/crates/v/rimhost.svg)](https://crates.io/crates/rimhost)
[![Documentation](https://docs.rs/rimhost/badge.svg)](https://docs.rs/rimhost)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](../LICENSE)

**`rimhost`** provides host operating system native storage tooling integration for the **[RIM](../README.md)** ecosystem (Layer 5).

While RIM operates rootless and in userspace by default, `rimhost` serves as an optional bridge when host-native filesystem drivers, kernel loopback mounts, or OS-level storage commands are explicitly requested (such as via the `--host` flag in `rimcli`).

---

## Features & Supported Platforms

- **Windows (`rimhost::windows`)**:
  Generates and orchestrates idempotent PowerShell automation scripts utilizing the Windows `Storage` and `Hyper-V` modules:
  - `New-VHD`, `Mount-VHD`, `Initialize-Disk`, `New-Partition`, `Format-Volume`, and `Dismount-VHD`.
- **Linux (`rimhost::linux`)**:
  Generates POSIX shell pipelines for kernel device mapping:
  - `losetup -Pf --show`, `parted -s`, `mkfs.vfat`, `mkfs.ext4`, `mkfs.ntfs`, and `mount -o loop`.
- **macOS (`rimhost::macos`)**:
  Generates macOS disk utility pipelines:
  - `hdiutil attach -nomount`, `diskutil partitionDisk`, `newfs_msdos`, and `hdiutil detach`.
- **Pre-Flight Validation**:
  Inspects the host `$PATH` via `FormatCommandBuilder` to ensure required system binaries are installed before attempting OS-level execution.

For deeper architectural details on native host integration, see **[Storage Synthesis & Transfer Engine](../docs/SYNTHESIS_AND_TRANSFER.md)** and the **[Architecture Overview](../docs/ARCHITECTURE.md)**.

---

## Installation

```bash
cargo add rimhost
```

---

## Usage Example

```rust
use rimgen::LayoutConfig;
use rimhost::format_inject_host;
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let layout = LayoutConfig::from_file(Path::new("layout.toml"))?;
    let image_path = Path::new("disk.img");

    // Execute host-native formatting and injection using OS utilities
    format_inject_host(&layout, image_path, false)?;

    println!("Host-native storage synthesis completed successfully.");
    Ok(())
}
```

---

## Related Documentation

- **[Storage Synthesis & Transfer Engine](../docs/SYNTHESIS_AND_TRANSFER.md)**
- **[Architecture Overview](../docs/ARCHITECTURE.md)**
- **[Workspace Overview](../README.md)**
