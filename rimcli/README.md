# rimcli

Unified command-line interface for the **RIM** (Rust Image Maker) storage toolkit.

Provides the **`rim`** executable.

[![Live Demo](https://img.shields.io/badge/Live%20Demo-mki.dev%2Frim-ff4081?style=flat-square&logo=googlechrome&logoColor=white)](https://mki.dev/rim)

> 🚀 **Try RIM live in your browser:** [mki.dev/rim](https://mki.dev/rim) — Interactive layout playground, real-time client-side WASM synthesis, and in-browser UEFI VM demo!

## Commands

- **`rim generate <layout.toml> [--output <path>]`** (aliases: `build`, `gen`): Synthesizes a disk image from a declarative layout.
- **`rim copy <src> <dst>`**: Direct logical file injection/extraction between host paths and disk image partitions (`image.img:1:/path`) without kernel mounting.
- **`rim convert <input> <output>`**: Converts between disk container formats (RAW, VHD, VMDK, QCOW2, VDI).
- **`rim inspect <image>`**: Deep inspection of image container format, partition scheme, and filesystem signatures.
- **`rim partition <image>`**: Displays GPT/MBR partition tables and LBA sector ranges.
- **`rim check <image>`**: Performs offline consistency checks on filesystems inside the image.

## Installation

```bash
cargo install --path .
```

## License

MIT License.

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
