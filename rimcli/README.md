# rimcli

Unified command-line interface for the **RIM** (Rust Image Maker) storage toolkit.

Provides the **`rim`** executable.

## Commands

- **`rim generate <layout.toml> [--output <path>]`** (aliases: `build`, `gen`): Synthesizes a disk image from a declarative layout.
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
