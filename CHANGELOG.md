# Changelog

All notable changes to the **RIM** (Rust Image Maker) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.0] - 2026-08-26
### Added
*   **Modular 12-Crate Architecture & Unified CLI (`rimcli`)**:
    *   `rimcli`: Unified CLI providing the binary `rim` with 5 powerful subcommands:
        *   `rim generate <layout.toml>` (aliases: `build`, `gen`): Declarative image generation with real-time UI spinners, layout validation tables, and colored status badges.
        *   `rim convert <input> <output>`: High-speed disk container conversion with live streaming progress bar, throughput (MB/s), and ETA calculation.
        *   `rim check <image>`: Offline filesystem integrity and metadata validation.
        *   `rim partition <image>`: GPT/MBR partition scheme inspector.
        *   `rim inspect <image>`: Deep container, partition, and filesystem detection with colored visual badges.
    *   `rimimg`: Standalone virtual machine container management crate supporting RAW (`.img`, `.raw`), Microsoft VHD Fixed (`.vhd`), VMware VMDK monolithicFlat (`.vmdk`), QEMU QCOW2 v2 (`.qcow2`), and VirtualBox VDI Fixed 1.1 (`.vdi`), format detection via magic bytes, wrap/unwrap, and direct conversions.
    *   `rimhost`: Isolated OS-native storage tools integration (Windows PowerShell Storage module, Linux `losetup`/`kpartx`/`mkfs.*`, macOS `hdiutil`/`diskutil`), accessible via the optional `--host` flag.
    *   `rimgen`: Refactored into a pure library-first declarative engine with `ImageBuilder`, direct stream synthesis (`build_on_io`), and typed event notifications (`BuildEvent`).
*   **Modular Multi-Crate Filesystem Engine (`rimfs`)**:
    *   `rimfs-core`: Shared core traits (`FsFormatter`, `FsAllocator`, `FsInjector`, `FsResolver`, `FsChecker`), common error types, macros (`bail!`, `ensure!`), bitmap utils, volume helpers, and `StdResolver`.
    *   `rimfs-fat`: Dedicated FAT12, FAT16, and FAT32 implementation, including the 64-bit optimized **RimFAT** extension specification (`RIMFAT_SPEC.md`).
    *   `rimfs-exfat`: Dedicated ExFAT implementation featuring allocation bitmap tracking, upcase table compilation, and directory entry walkers.
    *   `rimfs-ext`: Dedicated Ext2, Ext3, and Ext4 implementation with support for 48-bit physical extent trees, indirect block maps, and sparse superblock layouts.
    *   `rimfs-ntfs`: Pure-Rust, zero-dependency NTFS 3.1 filesystem engine supporting:
        *   MFT record parsing and generation (`FILE` records, fixup arrays / USA).
        *   Resident and non-resident attribute data runs encoding/decoding.
        *   System metadata streams (`$Boot`, `$MFT`, `$MFTMirr`, `$LogFile`, `$Volume`, `$AttrDef`, `$Bitmap`, `$BadClust`, `$UpCase`).
        *   Security Descriptor database management (`$Secure` with `$SDS`, `$SDH`, and `$SII` streams).
        *   B-tree directory indexing with `$INDEX_ROOT`, `$INDEX_ALLOCATION`, and index allocation bitmaps (`INDX`).
        *   `NtfsFormatter`, `NtfsAllocator`, `NtfsInjector`, `NtfsResolver`, and `NtfsChecker`.
*   **Enhanced Low-Level I/O Layer (`rimio`)**:
    *   `FileRimIO`: Specialized file I/O backend using OS-level positioned operations (`pread`/`pwrite` on Unix, `seek_read`/`seek_write` on Windows) for concurrency-safe, zero-seek random access.
    *   `MmapRimIO`: High-performance memory-mapped file I/O backend.
    *   `UefiRimIO`: Native UEFI block I/O backend with unaligned Read-Modify-Write buffering for bare-metal firmware execution.
    *   `test_suite`: Standardized reusable test suite validating `RimIO` trait implementations across backends.
*   **Forensic and Sector Analysis Tool (`sector-analyzer`)**:
    *   Signature detection scanner (NTFS, exFAT, EXT4, INDX, MFT `FILE`).
    *   Hexadecimal sector dump and entropy calculation per block.
    *   Sector-by-sector disk diffing and NTFS-specific inspection commands (dump MFT, SDS, and boot structures).

### Changed
*   **Decoupled Architecture**: Completely separated the declarative engine (`rimgen`), container formats (`rimimg`), OS host fallbacks (`rimhost`), and user interface (`rimcli`).
*   **`rimfs` Public Facade**: Refactored `rimfs` into a thin, feature-gated facade crate re-exporting `rimfs-core`, `rimfs-fat`, `rimfs-exfat`, `rimfs-ext`, and `rimfs-ntfs`.
*   **Workspace Optimization**: Unified workspace configuration with Rust edition 2024 and shared dependency specifications across all 12 crates.

## [0.5.1] - 2026-01-18
### Fixed
*   **EXT4 e2fsck Compatibility**: resolved multiple consistency errors including:
    *   Set `EXT4_FEATURE_INCOMPAT_FILETYPE` flag in superblock.
    *   Corrected directory entry `rec_len` calculations and block spanning.
    *   Fixed parent directory link counts.
    *   Synchronized block/inode bitmaps and free counts.
*   **Code Cleanup**: Refactored `Ext4Injector` to use `RimIO` primitive writers, removing manual byte packing.

## [0.5.0] - 2026-01-17
### Added
*   **EXT4 Support**: Full read/write implementation for EXT4 filesystems.
    *   `Ext4Formatter`: Capability to format volumes with proper superblock and block group descriptors.
    *   `Ext4Injector`: Support for injecting files and directories, handling extents and directory entries.
    *   `Ext4Checker`: robust filesystem consistency checker.
*   **Documentation**: Added comprehensive `README.md` and `CHANGELOG.md`.

### Changed
*   **Version Bump**: Project version updated to 0.5.0 to reflect maturity.

## [0.4.0]
### Added
*   **`rimpart`**: New crate for handling GPT partition tables.
*   **`rimgen`**: New high-level crate for orchestrating disk image generation.

## [0.3.0]
### Added
*   **ExFAT Support**: Implementation of the ExFAT filesystem.
    *   Allocation Bitmap management.
    *   Upcase table support.
    *   Large file support.

## [0.2.0]
### Added
*   **FAT32 Support**: Basic implementation of the FAT32 filesystem.
    *   FAT chain traversal and manipulation.
    *   Standard directory entry handling.

## [0.1.0]
### Added
*   **Initial Release**: Foundation of the project.
*   **`rimio`**: Core IO traits (`BlockIO`) and abstractions for memory and file-based access.
