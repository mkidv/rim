# RIM (Rust Image Maker)

**RIM** is a comprehensive, pure-Rust toolkit for generating, manipulating, and analyzing disk images and filesystems. It is designed for high reliability, `no_std` embedding, and ease of use.

[![CI](https://github.com/mkidv/rim/actions/workflows/ci.yml/badge.svg)](https://github.com/mkidv/rim/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/rimgen.svg)](https://crates.io/crates/rimgen)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Ecosystem

The project is divided into several composable crates:

| Crate | Description |
|---|---|
| **[`rimgen`](rimgen)** | High-level CLI tool and library to generate disk images declaratively (`layout.toml`). |
| **[`rimfs`](rimfs)** | Filesystem implementations (FAT32, ExFAT, EXT4) with `no_std` support. |
| **[`rimpart`](rimpart)** | Partition table manipulation (GPT, MBR) and streaming readers. |
| **[`rimio`](rimio)** | Core I/O traits and abstractions (Blocking, Async-ready, UEFI/Std/Alloc support). |

## Installation

To install the CLI tool `rimgen`:

```bash
cargo install rimgen
```

## Usage

Create a `layout.toml` file:

```toml
[[partitions]]
name = "boot"
size = { Fixed = 128 }
fs = "Fat32"
bootable = true

[[partitions]]
name = "root"
size = "auto"
fs = "Ext4"
```

Generate the image:

```bash
rimgen layout.toml --output disk.img
```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Repository

Source code is available at: [https://github.com/mkidv/rim](https://github.com/mkidv/rim)
