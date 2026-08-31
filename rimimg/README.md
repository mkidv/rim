# rimimg

Virtual machine disk container and image formats for the **RIM** ecosystem.

## Supported Formats

- **RAW**: `.img`, `.raw`
- **Microsoft VHD**: `.vhd` (fixed VHD)
- **VMware VMDK**: `.vmdk` (monolithicFlat)
- **QEMU QCOW2**: `.qcow2` (v2/v3 header)
- **VirtualBox VDI**: `.vdi` (fixed 1.1)

## Features

- Magic bytes and extension format detection (`ImageFormat::from_extension`, `from_io`).
- `no_std` core APIs over `RimRead`/`RimWrite`, with `alloc` only where container metadata requires heap buffers.
- Logical raw-disk adapters for image containers (`create_image_io`, `open_image_io`).
- Container wrapping and unwrapping over caller-provided I/O streams (`wrap_io`, `unwrap_io`, plus `*_with_progress` variants).
- File/path orchestration lives in CLI and native tooling crates.

## License

MIT License.

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
