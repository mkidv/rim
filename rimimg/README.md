# rimimg

Virtual machine disk container and image formats for the **RIM** ecosystem.

## Supported Formats

- **RAW**: `.img`, `.raw`
- **Microsoft VHD**: `.vhd` (fixed VHD)
- **VMware VMDK**: `.vmdk` (monolithicFlat)
- **QEMU QCOW2**: `.qcow2` (v2/v3 header)
- **VirtualBox VDI**: `.vdi` (fixed 1.1)

## Features

- Magic bytes and extension format detection (`ImageFormat::from_file`, `from_path`, `from_io`).
- Container wrapping and unwrapping (`wrap`, `unwrap`).
- Direct inter-format conversions with progress reporting (`convert`, `convert_with_progress`).

## License

MIT License.
