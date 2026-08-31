# rimhost

OS-native storage tools integration for the **RIM** ecosystem.

Provides fallback execution drivers for native operating system storage commands:
- **Windows**: PowerShell & Storage module (`Mount-VHD`, `Format-Volume`, `Dismount-VHD`)
- **Linux**: `losetup`, `kpartx`, `mkfs.*`, `mount`
- **macOS**: `hdiutil`, `diskutil`

This crate is used as an optional fallback in `rimcli` via the `--host` flag.

## License

MIT License.

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
