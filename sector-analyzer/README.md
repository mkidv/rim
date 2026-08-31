# sector-analyzer

`sector-analyzer` is a forensic analysis, debugging, and low-level inspection CLI tool for disk images and filesystems in the RIM ecosystem.

## Features

- **Filesystem Signature Scanning**: Detects partition headers, boot sectors, MFT `FILE` records, and B-tree `INDX` structures.
- **Sector Dumps & Hex Viewer**: Formats LBA sectors with combined hex and ASCII outputs.
- **NTFS Forensic Inspection**:
  - `dump-mft`: Parses and dumps individual MFT records by index.
  - `dump-secure`: Traverses `$Secure` metadata streams (`$SDS`, `$SDH`, `$SII`) and decodes security descriptors.
- **Block-Level Entropy**: Calculates Shannon entropy across disk regions to identify compressed or encrypted data.
- **Sector-by-Sector Disk Diff**: Identifies exact divergent sectors between two disk images.

## CLI Usage

```bash
# Scan disk image for known filesystem signatures
sector-analyzer signatures disk.img

# Dump 4 sectors starting at LBA 2048
sector-analyzer dump-lba disk.img 2048 --count 4

# Dump NTFS MFT record #0 ($MFT) or record #9 ($Secure)
sector-analyzer dump-mft disk.img 0

# Inspect NTFS Security Descriptors
sector-analyzer dump-secure disk.img

# Calculate entropy across 64 KiB blocks
sector-analyzer entropy disk.img --chunk 65536

# Find hex pattern or string across disk
sector-analyzer find disk.img "0xEB5890"

# Compare two disk images sector by sector
sector-analyzer diff disk_v1.img disk_v2.img
```

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
