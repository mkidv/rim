# Changelog

All notable changes to the **RIM** (Rust Image Maker) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.6.2] - 2026-08-27
### Fixed
*   **NTFS Root Directory Index Stream Corruption & Duplicate Terminator (`rimfs-ntfs`)**:
    *   Fixed `NtfsMftRecord::new_dir` to avoid appending duplicate `LAST_ENTRY` terminators to already-terminated `$INDEX_ROOT` entry streams, resolving `Corrupt index entry stream in inode 5` and secondary `$Secure` path lookup failures in `ntfsfix`.
    *   Fixed `IndexTreeBuilder::build` child pointer VCN calculations: replaced raw block ordinals (`0, 1, 2...`) with cluster-scaled VCNs (`0, 8, 16...` for 512B clusters with 4KB INDX blocks).
    *   Added generic VCN conversion and index cluster sizing helpers to `NtfsMeta` (`clusters_per_index_record_raw`, `index_block_to_vcn`, `vcn_to_index_block`, `total_clusters_for_index_blocks`).
    *   Fixed `NtfsResolver` INDX block parsing: corrected `IndexNodeHeader` offset to 24 (`core::mem::size_of::<IndexRecordHeader>()`), switched from `apply_usa_fixup` to `decode_usa_fixup`, and added entry length bounds safety.

### Added
*   **Deep NTFS B-Tree & Allocation Structural Verification (`NtfsChecker`)**:
    *   Added `check_directory_index` for Inode 5 (`$Root`) and recursive directory traversal in `check_cross_reference`.
    *   `IDX.ROOT`: Structural verification of `$INDEX_ROOT:$I30` (8-byte entry alignment, bounds, child VCNs, strict termination, and rejection of trailing entries).
    *   `IDX.ALLOC`: Validates `$INDEX_ALLOCATION:$I30` INDX records, USA fixup integrity, and header VCN agreement with calculated cluster VCNs.
    *   `IDX.BITMAP`: Validates `$BITMAP:$I30` bit allocation against physical INDX records.
    *   `IDX.VCN` & `IDX.CROSSREF`: Cross-references child VCN pointers to ensure every referenced subnode resolves to a valid, unique INDX block.
    *   Added comprehensive regression tests for multi-block index trees (512B/4KB, 4KB/4KB geometries) and a deliberate index corruption tripwire test.

## [0.6.1] - 2026-08-27
### Added
*   **End-to-End POSIX Metadata & Symlink Engine (`rimfs-core`, `rimfs-ext`)**:
    *   `NodeKind` Enum & Granular Attributes: Replaced boolean directory flags with `NodeKind` (`Regular`, `Directory`, `Symlink`, `Fifo`, `Socket`, `CharDevice`, `BlockDevice`) and explicit 32-bit `uid`/`gid: Option<u32>` on `FileAttributes`.
    *   `FsNode::Symlink` & Extended Node Counts: First-class symbolic link tree representation and tracking in `FsNodeCounts` (`symlinks: usize`).
    *   `FsTreeResolver` & `FsTreeInjector` Abstractions: Added `write_symlink` and `read_link` pipeline methods with typed `FsInjectorError::Unsupported` error handling for filesystems without symlink support.
    *   `StdResolver` Host Discovery: Resolves Unix permissions (`0o7777` mask, `setuid`, `setgid`, `sticky`), ownership (`uid`, `gid`), and symlink targets via `fs::symlink_metadata()` and `read_link()`.
    *   EXT Fast & Slow Symlinks:
        *   **Fast Symlinks** (< 60 bytes): Embedded directly into `ExtInode.i_block` with zero block allocation and `EXT_INODE_FLAG_EXTENTS` cleared.
        *   **Slow Symlinks** (>= 60 bytes): Allocated data block(s) backed by 48-bit extent trees (Ext4) or indirect block maps (Ext2/3).
    *   EXT 12-Bit Modes & 32-Bit UID/GID: Full support for `SETUID (0o4000)`, `SETGID (0o2000)`, and `STICKY (0o1000)` bits, plus 32-bit UID/GID on-disk encoding (`i_uid`/`i_gid` + `i_osd2[4..8]`).
    *   `ExtChecker` Symlink Invariants: Validation for fast/slow symlink sizing, extent flags, and block count consistency.
    *   Linux CI Semantic Integrity: Extended loop device mounting in `.github/workflows/ci.yml` to assert POSIX directory modes, special bits, fast/slow symlinks, and zero-block allocation using native `stat` and `readlink`.

### Fixed
*   **NTFS `$UpCase` MFT Record 10 Defect (`rimfs-ntfs`)**:
    *   Allocated and encoded the complete 131,072-byte uppercase table as a valid non-resident `$DATA` stream in MFT Record 10 with proper cluster dataruns and logical/allocation sizes.
    *   Updated `$FILE_NAME` attribute generation to record accurate `allocated_size` and `data_size` for `$UpCase`.
    *   Extended `NtfsChecker` and regression test suite to inspect Record 10 non-residency, 131,072-byte sizing (`UPCASE.SIZE`), and table integrity (`UPCASE.DATA`).
*   **EXT Directory Attribute Preservation (`rimfs-ext`)**:
    *   Fixed `ExtInjector::flush_current()` to preserve directory modes (e.g. `0o700`), custom ownership, and timestamps when rewriting directory inodes on child completion.
*   **Zero-Block Allocation Optimization (`rimfs-ext`)**:
    *   `ExtAllocator::allocate_blocks_list` returns an empty `RunList` immediately for 0-block requests (used by fast symlinks).

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
