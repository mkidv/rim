# sector-analyzer

`sector-analyzer` is a universal forensic analysis, low-level inspection, and verification CLI tool for virtual disk images and filesystem structures in the RIM ecosystem.

It operates entirely in user space without requiring operating system drivers, loopback devices, or administrative privileges to mount images.

## Features

- **Container Detection (`rimimg`)**: Automatically identifies RAW, VHD, VMDK, VDI, and QCOW2 virtual container headers and offsets.
- **Partition Table Scanning (`rimpart`)**: Automatically decodes GPT (GUID Partition Tables) and legacy MBR structures, reporting offsets, partition sizes, and filesystem signatures.
- **Universal Filesystem Verification (`FsChecker`)**: Runs RIM's deep filesystem checker engines directly across disk partitions:
  - NTFS (`NtfsChecker`)
  - FAT12 / FAT16 / FAT32 (`FatChecker`)
  - exFAT (`ExFatChecker`)
  - EXT2 / EXT3 / EXT4 (`ExtChecker`)
  - ISO 9660 / Joliet (`IsoChecker`)
- **Direct Filesystem Traversal (`FsTreeResolver`)**: Lists directory structures and extracts file payloads (`ls`, `cat`) directly from raw images without mounting.
- **Format-Specific Forensic Dumps**:
  - `dump-fat-meta`: Decodes FAT12/16/32 BPB, geometry, FSInfo, and exFAT boot sectors.
  - `dump-ext-meta`: Decodes EXT4 Superblock, feature flags, block groups, and Block Group Descriptors.
  - `dump-ntfs-meta` & `dump-mft`: Decodes MFT records, attribute streams, and runlists.
  - `dump-secure`: Traverses `$Secure` metadata streams (`$SDS`, `$SDH`, `$SII`) and decodes Windows security descriptors.
- **Forensic Utilities**:
  - `probe`: Scans for known filesystem signatures and partition headers.
  - `entropy`: Computes block-level Shannon entropy to locate encrypted or compressed areas.
  - `diff`: Performs sector-by-sector binary comparisons between two disk images.
  - `find`: Searches for hex sequences or ASCII strings across raw sectors.
  - `dump-lba` & `extract-bin`: Hex dump or carve out exact sector ranges.

## CLI Usage

### Universal Forensic Commands

```bash
# Automatic container, partition table, and filesystem detection
sector-analyzer disk.img
sector-analyzer info disk.vmdk

# Run deep FsChecker on partition 0 (or specify index)
sector-analyzer check disk.img 0

# List directory contents without OS mounting
sector-analyzer ls disk.img 0 /
sector-analyzer ls disk.img 0 /EFI/BOOT

# Extract file contents directly
sector-analyzer cat disk.img /EFI/BOOT/BOOTX64.EFI 0 > bootx64.efi
sector-analyzer cat disk.img /etc/os-release 0
```

### Format-Specific Metadata Dumps

```bash
# Dump FAT12/16/32 or exFAT boot parameters
sector-analyzer dump-fat-meta disk.img 0

# Dump EXT4 Superblock and Block Group Descriptors
sector-analyzer dump-ext-meta disk.img 0

# Dump NTFS general metadata
sector-analyzer dump-ntfs-meta disk.img

# Dump specific NTFS MFT record (e.g. record 0 for $MFT, 9 for $Secure)
sector-analyzer dump-mft disk.img 0

# Traverse NTFS security descriptors in $Secure
sector-analyzer dump-secure disk.img
```

### Low-Level Hex & Carving Tools

```bash
# Scan image for known filesystem signatures
sector-analyzer probe disk.img

# Hex dump 4 sectors starting at LBA 2048
sector-analyzer dump-lba disk.img 2048 4

# Extract 100 sectors starting at LBA 2048 to a raw binary file
sector-analyzer extract-bin disk.img 2048 100 partition1_vbr.bin

# Calculate Shannon entropy across 64 KiB blocks
sector-analyzer entropy disk.img 65536

# Search for hex byte sequences or ASCII strings
sector-analyzer find disk.img "0xEB5890"

# Compare two disk images sector by sector
sector-analyzer diff disk_v1.img disk_v2.img
```

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
