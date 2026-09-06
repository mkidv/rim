# Changelog

All notable changes to the **RIM** (Rust Image Maker) project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.9.0] - 2026-09-06
### Added
*   **Universal Filesystem Copy Engine (`rim copy`)**:
    *   New high-performance subcommand in `rimcli` for logical, streaming transfers between host filesystems and disk image partitions (or between two disk images) without requiring mounting or root privileges.
    *   Symmetric endpoint addressing supporting host paths (`/path/to/dir`) and image partitions (`disk.img:<part>:/path` or raw filesystems).
    *   Comprehensive policy controls:
        *   `--overwrite <POLICY>`: `error`, `skip`, or `replace` (in-place entry replacement on Host).
        *   `--metadata <POLICY>`: `best-effort`, `preserve`, or `ignore` (permissions, timestamps, mode).
        *   `--unsupported <POLICY>`: `warn`, `error`, or `skip` (gracefully handling unsupported filesystem features such as symlinks on FAT).
    *   Destination-aware case-collision detection respecting the sensitivity of the destination filesystem (strict collision errors on FAT/NTFS/Windows host; case-preserving coexistence on Ext4/Unix).
    *   Interactive terminal progress reporting with transfer speed, ETA, and copy summary statistics.
*   **In-Memory Dry-Run Simulation (`--dry-run`)**:
    *   Dry-run mode operates strictly in memory with zero disk writes and zero file creation on destination hosts or images.
    *   `OverlayRimIO` / `PagedOverlayRimIO` in `rimio`: sparse copy-on-write page overlay wrapping any read-only base storage in memory, enabling real filesystem injectors (`ExtInjector`, `FatInjector`, `NtfsInjector`, etc.) to run their allocation and verification algorithms purely in memory.
    *   `DryRunStdInjector` in `rimcli`: performs path confinement, file overwrite detection, and payload stream verification for host destinations without disk mutation.
    *   Reports dry-run statistics including simulated file counts, data streamed, and allocated overlay RAM.
*   **`StdInjector` Host Streaming Injection (`rimfs-core`)**:
    *   New `FsTreeInjector` implementation in `rimfs-core` (under `feature = "std"`) providing symmetric write-side host injection matching `StdResolver`.
    *   Streams `RimRead` sources directly to host files, supports directory hierarchies, preserves representable metadata and permissions, and strictly confines writes within the destination root through symlink-boundary checks.
*   **Unpartitioned Volume Generation (`--no-gpt` / `BuildOptions`)**:
    *   Added `--no-gpt` (alias `--nogpt`) flag to `rim generate` to synthesize raw unpartitioned filesystem images without GPT or protective MBR partition tables.
    *   Added `BuildOptions` and `PartitionTable::{Gpt, None}` to `rimgen`, supporting raw filesystem layouts at LBA 0.
*   **NTFS Modernization & Windows 11 Compliance (`rimfs-ntfs`)**:
    *   Added Windows 11 CHKDSK and kernel compliance regression test suite (`tests/windows_compliance.rs`) locking down on-disk structures (exact 2560-byte `$AttrDef` table, root DACL, `$Bitmap` consistency, and `UpCase:$Info`).
    *   Added NTFS `$Extend/$Quota` system files, index layout, and SID collation support.
    *   Modularized the checker subsystem into dedicated checkers for records, indexes, bitmaps, and system structures.
    *   Modularized builder internals into dedicated `features/` and typed structures (`attrdef`, `attribute`, `collation`, `index_layout`, `mft`, `quota`, `sid`, `security`).
    *   Upgraded `NtfsInjector` with robust INDX B-Tree directory allocation and attribute management.
*   **EXT4 Run-Coalescing & Superblock Flushing (`rimfs-ext`)**:
    *   Upgraded `Ext4BlockAllocator` to search for and allocate contiguous runs of free blocks, significantly accelerating multi-block allocations and reducing fragmentation.
    *   Added `flush_superblock` and `flush_bgdt` to write free block/inode counters and directory counts across primary and sparse backup superblocks and block group descriptor tables.
    *   Modularized internal layout and types into `types/flags.rs`, `types/group_layout.rs`, and `utils/dir.rs`.
*   **Cross-Filesystem Transfer Matrix & Memory Optimizations**:
    *   Added comprehensive `rimfs/tests/cross_fs_transfer.rs` integration suite verifying bidirectional tree transfers across all 7 supported engines (FAT, ExFAT, EXT, NTFS, TAR, ZIP, ISO).
    *   Reduced heap allocations in directory resolvers (`FatResolver`, `ExFatResolver`) by reusing buffers across directory runs in `read_dir_entries` and `find_in_dir`.
    *   Unified path resolution via `walk_path` across filesystems.
    *   Made `zero_cluster_heap` generic over cluster unit types (`U: Copy + Ord + Into<u64>`).
    *   Implemented `FsHandle` for `()`.

### Changed
*   Bumped workspace crates to version `0.9.0`.
*   Preserved `no_std + alloc` compatibility across `rimfs-core` and `rimio`.

## [0.8.2] - 2026-09-02
### Added
*   `rimfs-iso` now supports streaming `FsTreeResolver` injection while still building the complete ISO layout plan before payload serialization.

### Changed
*   Kept the ISO layout planner internal to `rimfs-iso` instead of exporting `IsoLayoutPlan` through the public API, traits, or prelude.
*   Updated ISO documentation to describe precomputed layout as an internal implementation detail.

## [0.8.1] - 2026-09-01
### Added
*   Added read-only image container opening in `rimimg` through `ImageReadIO` and `open_image_read_io`.
*   Added `ImageFormat::from_read` for format detection on read-only `RimRead` streams.
*   Added `FsTreeInjector::inject_tree_from_resolver` for bounded-memory tree injection from resolvers.

### Changed
*   Clarified `FsTreeResolver::read_file` as an explicit full-file materialization helper backed by `open_file`.
*   Removed redundant filesystem-specific `read_file` implementations so file reads share the streaming `open_file` path.
*   `rimpart` GPT/MBR scanning APIs now accept `RimRead` when mutation is not required.
*   `rimgen` layout-config mountpoints now inject directly from `StdResolver` instead of resolving host files into `VecRimIO` snapshots.
*   The WebAssembly inspect demo now reads uploaded RAW, VHD, VMDK, QCOW2, and VDI images through `rimimg` and reports container/logical sizes.
*   Browser inspection now uses `SliceRimIO` directly, avoiding an extra mutable in-memory image copy.
*   Simplified `rimgen` injection helpers.

## [0.8.0] - 2026-08-31
### Added
*   **ZIP Archive Engine (`rimfs-zip`)**:
    *   New dedicated `no_std + alloc` ZIP filesystem driver implementing the complete `rimfs` pipeline (`Zip`, `ZipFormatter`, `ZipAllocator`, `ZipInjector`, `ZipResolver`, `ZipChecker`).
    *   Supports Local File Headers, Central Directory, EOCD/ZIP64 structures, Store/Deflate plumbing, CRC-32 verification, and Info-ZIP POSIX metadata.
    *   Integrated into the `rimfs` facade and `rimgen` feature matrix via the `zip` cargo feature.
*   **ISO 9660 / Joliet / Rock Ridge / El Torito Engine (`rimfs-iso`)**:
    *   New pure-Rust optical and hybrid disk driver with precomputed layout planning, 2048-byte sector serialization, descriptor/checker support, and lazy file resolution.
    *   Added Joliet UTF-16 names, Rock Ridge POSIX metadata and symlink records, plus El Torito BIOS/UEFI boot catalog support.
    *   UEFI ISO images can synthesize an embedded FAT EFI boot image in memory through `rimfs-fat`.
*   **Lazy Extent-Based File Streaming (`rimio`, `rimfs-*`)**:
    *   Added `IoExtent` and `ExtentRimRead` for sparse/contiguous file views backed by existing `RimRead` sources.
    *   FAT, ExFAT, EXT, NTFS, TAR, ISO, and ZIP resolvers now expose lazy `open_file` readers instead of eagerly copying file payloads into memory.
    *   Added nested archive/disk smoke coverage demonstrating ISO-in-ISO traversal without full payload materialization.
*   **Cross-Engine Benchmarks & Examples**:
    *   Added dedicated TAR, ZIP, ISO, RimFAT, and all-engines comparison Criterion benchmarks.
    *   Added executable examples for TAR, ZIP, and ISO alongside refreshed existing filesystem examples.
*   **`rimimg` `no_std` Core**:
    *   Added a `no_std` container-format core with explicit `alloc`/`std` features, typed `RimImgError`, deterministic image options, and direct `RimRead`/`RimWrite` APIs.
    *   Added logical image I/O adapters (`create_image_io`, `open_image_io`) so callers can read or write a raw disk view directly over RAW, VHD, VMDK, QCOW2, and VDI containers.
    *   Moved host file/path orchestration out to CLI/native callers so `rimimg` remains focused on container I/O primitives.
*   **Sparse Release Torture Coverage**:
    *   Added `SparseRimIO` / `PagedSparseRimIO` for multi-TiB logical storage tests and dry-run image generation without materializing zero-filled regions.
    *   Added a multi-filesystem sparse torture layout crossing 2 TiB / 4 TiB boundaries and validating far-offset GPT/filesystem synthesis in dry-run mode.
*   **Support Matrix Documentation**:
    *   Added a concise repository-level support matrix covering format, inject, resolve/read, fast check, deep check, `no_std + alloc`, and known limitations per filesystem/archive driver.

### Changed
*   **Read/Write API Split (`rimio`)**:
    *   Split `RimIOExt` helpers into `RimReadExt` and `RimWriteExt`, with matching primitive and zerocopy struct helper traits.
    *   Re-exported the expanded prelude so read-only resolvers can use primitive/struct helpers without requiring writable I/O.
*   **Resolver Trait Lifetimes (`rimfs-core`)**:
    *   Removed the lifetime parameter from `FsTreeResolver` and made `open_file` borrow from the resolver call site.
    *   `resolve_node`, `resolve_tree`, and `resolve_entry` now return owned node trees with buffered `VecRimIO` sources when a generic tree snapshot is required.
*   **Read-Only Metadata & Resolver Paths (`rimfs-*`)**:
    *   `from_io` metadata constructors and filesystem resolvers now accept `RimRead` where mutation is not required.
    *   FAT chain walking gained read-only `get_ro` support to avoid unnecessary dirty-buffer flushes during resolution.
*   **Workspace Expansion**:
    *   Workspace expanded from 13 to 15 decoupled crates and default filesystem features now include `tar`, `zip`, and `iso`.
*   **Filesystem Examples & Benches Layout**:
    *   Moved filesystem-specific examples and Criterion benches into their owning `rimfs-*` crates.
    *   Kept per-example timing and `IOCounter` statistics for format, inject, check, and resolve phases.
    *   Kept only the all-engines comparison benchmark in the `rimfs` facade crate.
*   **Public API Surface**:
    *   Reduced filesystem crates to root/prelude exports for high-level APIs while keeping low-level on-disk `types` modules available for inspection.
    *   Kept NTFS `view` helpers public as the dedicated advanced inspection API.
    *   Added `std::error::Error` implementations for partition error types under `std`.
*   **Direct Container Generation (`rimgen`)**:
    *   `rimgen` now builds non-raw image outputs through `rimimg` container-backed I/O instead of creating a temporary raw image and wrapping it afterwards.
*   **Sparse Dry-Run Generation (`rimcli`, `rimgen`, `rimio`)**:
    *   `rim generate --dry-run` now executes the normal layout/build pipeline over sparse in-memory storage, reporting logical size, allocated bytes, and allocated pages when verbose.
    *   Large declarative layouts now exercise GPT, partition offsets, filesystem formatters, injectors, and checkers without requiring temporary multi-TiB host files.
*   **EXT4 Large Geometry Handling (`rimfs-ext`)**:
    *   `GroupLayout` now carries physical block numbers as `u64`, matching `ExtMeta::block_count` and EXT4 64-bit superblock/BGDT fields.
    *   Default block groups now derive from the block bitmap capacity (`block_size * 8`) while preserving the existing 16 KiB-per-inode policy.

### Fixed
*   **Pre-Release Safety Audit**:
    *   Hardened `rimio` partition-offset arithmetic across memory, file, mmap, and UEFI backends to reject overflow instead of wrapping in release builds.
    *   Routed `zero_fill()` through `RimWrite::zero_at()` so sparse-capable backends can reclaim or skip zero pages instead of materializing large zero buffers.
    *   Guarded ZIP, ISO, and NTFS resolvers/checkers against malformed on-disk sizes and offsets before allocating buffers or exposing file extents.
    *   Disabled physical UEFI disk writes by default in `uefi-synth`; provisioning now requires the explicit `dangerous-uefi-write` feature.
    *   Fixed default `uefi-synth` compilation by gating the physical provisioning and chainloading path behind `dangerous-uefi-write`.
    *   Fixed packaged `rimgen` builds by importing `RimWriteExt` where `zero_fill` is used outside the local workspace context.
    *   Restored `rimfs-zip` `no_std + alloc` builds and removed module-level `no_std` attributes that only belong at crate root.
    *   Kept NTFS spec-compliance tests private to the crate instead of exporting them through the public API.
    *   Fixed `rimio --no-default-features` by exposing sparse storage only when `alloc` is enabled.
*   **FAT Mirror Verification (`rimfs-fat`, `rimfs-core`)**:
    *   Fixed `compare_fat_copies` to compare FAT0 against FAT1 instead of reading FAT0 twice through the shared driver cache.
    *   Added explicit per-FAT table reads to `FatDriver`, preserving mirrored writes while allowing checkers to validate individual FAT copies.
    *   Added a corruption tripwire that formats a two-FAT volume, corrupts only FAT1, and verifies that `FAT.MIRROR` is reported.
*   **EXT4 Multi-TiB Geometry (`rimfs-ext`)**:
    *   Fixed overflow panics in sparse superblock and backup BGDT offset calculations on multi-TiB layouts.
    *   Serialized 64-bit BGDT block pointers using low/high fields for block bitmaps, inode bitmaps, and inode tables.
    *   Updated EXT checks to read 64-bit block counts and BGDT block pointers instead of validating only the low 32 bits.
    *   Added a block-count boundary test proving that `u32::MAX + 1` EXT4 blocks are represented through `s_blocks_count_hi`.
    *   Switched the standard inode scan to inode bitmaps, leaving the full inode-table scan as an explicit deep path.
*   **FAT Checker Modes (`rimfs-fat`)**:
    *   Fixed `deep_walk` to include the last valid data cluster and reuse a single FAT driver cache during sequential scans.
    *   Kept `fast_check()` bounded by disabling the full FAT chain walk while retaining boot, root, and mirror checks.
*   **Injection Finalization in Examples**:
    *   Existing filesystem examples now explicitly call `injector.flush()` before validation/resolution.
    *   NTFS example persists a raw test image under `target/ntfs_test.img` for native-tool verification.
*   **Rustdoc Readiness**:
    *   Fixed rustdoc warnings under `RUSTDOCFLAGS="-D warnings"`, including bare URL handling in NTFS security type documentation.
*   **Development Artifacts**:
    *   Removed the external FAT32 comparison benchmark and its `fatfs`/`fscommon` dev-dependencies.
    *   Moved generated example fixtures and their generator out of packaged crates into `scratch/`.
*   **NTFS Resolver Hot Paths (`rimfs-ntfs`)**:
    *   Added MFT record and directory-entry caches plus persistent upcase handling to reduce repeated parsing during path traversal.
    *   `NtfsInjector::new` now initializes allocation state from the existing volume and reserves system records through `$UsnJrnl`.
*   **TAR Ergonomics (`rimfs-tar`)**:
    *   Added `TarMeta::new(...)` and switched TAR file opening to contiguous extent-backed readers.

## [0.7.0] - 2026-08-29
### Added
*   **POSIX UStar TAR Archive Engine (`rimfs-tar`)**:
    *   New dedicated `no_std + alloc` TAR filesystem driver implementing the complete `rimfs` pipeline (`Tar`, `TarFormatter`, `TarAllocator`, `TarInjector`, `TarResolver`, `TarChecker`).
    *   Standard UStar 512-byte header serialization, octal integer encoding/decoding, header checksum validation, and end-of-archive (`2 x 512` zero bytes) management.
    *   Integrated into the `rimfs` public facade via the `tar` cargo feature.
*   **WebAssembly & UEFI In-Memory Synthesis Demos (`examples/wasm-synth`, `examples/uefi-synth`)**:
    *   `wasm-synth`: Standalone WebAssembly synthesis module (`wasm32-unknown-unknown`) producing valid multi-partition GPT images in RAM with real-time browser inspection UI.
    *   `uefi-synth`: Bare-metal UEFI application (`x86_64-unknown-uefi`) running under `#![no_std]` without standard library.
    *   Automated release web asset pipeline via `scripts/update_web_assets.ps1`.
*   **NTFS File Attributes Extension & Deep Metadata Resolution (`rimfs-ntfs`)**:
    *   Introduced `NtfsFileAttributesExt` trait (`as_ntfs_attr()`, `from_ntfs_attr()`) implemented on `FileAttributes`, re-exported in `prelude` and `traits`.
    *   Enhanced `NtfsResolver::read_attributes` to parse `$STANDARD_INFORMATION` (type `0x10`) resident attributes, decoding file flags and Windows FILETIME timestamps (`created`, `modified`, `accessed`) into `OffsetDateTime`.
    *   Transparent resolution for named (`$I30`) and unnamed `$INDEX_ROOT` and `$INDEX_ALLOCATION` directory streams.
*   **Bounded I/O Streams (`rimio`)**:
    *   Introduced `BoundedRimIO` for zero-overhead bounded partition slicing and isolated sub-stream operations.
    *   Enhanced `IOCounter` metrics with alignment tracking and snapshotting.
*   **Declarative Engine & Layout Invariants (`rimgen`)**:
    *   Added minimum volume size boundary `Filesystem::Ntfs if size_mb < 4` in `check_size_limit`.
    *   Full support for `part.uuid` (hex 64-bit/32-bit `1234-5678` or UUID 128-bit) in `format_inject_ntfs`.
    *   Generalization of error macros (`ensure!`, `bail!`) via `gen_error_wiring!`.
    *   Added multi-filesystem portable in-memory synthesis and layout equivalence tests (`portable_in_memory_test.rs`, `identical_engine_equivalence_test.rs`).

### Changed
*   **Filesystem Drivers API Harmonization (`rimfs-*`)**:
    *   Uniformized driver identifiers across all crates in lowercase: `fat`, `exfat`, `ext4`, `ntfs`, `tar`.
    *   Renamed `TarFs` directly to `Tar`, exporting standard `traits` and `prelude` submodules in `rimfs-tar`.
    *   Made `ExtMeta::new(...)`, `new_ext2`, `new_ext3`, `new_custom` fallible (`FsResult<ExtMeta>`) with strict geometry validation (power-of-two block size 1KB-64KB, block count $\ge 16$, inodes per group $> 0$).
    *   Made `ExtInjector::new(...)` and `TarInjector::new(...)` fallible (`FsInjectorResult<Self>`).
    *   Renamed partition layout structures: `ResolvedPartition` $\rightarrow$ `Partition`, and raw configuration input to `PartitionConfig`.
*   **Modular Ecosystem Expansion**:
    *   Workspace expanded from 12 to 13 decoupled crates.

### Fixed
*   **EXT4 Bit-Exact Deterministic Synthesis (`rimfs-ext`)**:
    *   Defaulted timestamps to `OffsetDateTime::UNIX_EPOCH` instead of `now_utc()` when `None` in `ExtInode::from_attr` and fast symlinks, ensuring 100% reproducible bit-for-bit synthesis.
*   **Cleaned Repository Noise**:
    *   Removed unused `rimgen::utils::string` dead code, legacy layout data files, and empty `rimfs-ntfs/src/features` directory.

## [0.6.3] - 2026-08-27
### Fixed
*   **NTFS Native Tool Compatibility & Mount Support (`rimfs-ntfs`)**:
    *   **Alternate Boot Sector Location**: Derived canonical `backup_boot_sector_offset` in `NtfsMeta` (`total_sectors * bytes_per_sector`), placing the backup VBR at the exact partition boundary sector as expected by Windows and `ntfsfix`.
    *   **Data Run Positive Integer Encoding**: Fixed variable-length integer serialization in `encode_variable_int_to_buf` for unsigned lengths (e.g. 128 clusters = `0x80`), appending an MSB-cleared byte (`[0x80, 0x00]`) to ensure the signed NTFS runlist decompressor does not misinterpret lengths as negative values (`EIO` in `ntfs_attr_pread`).
    *   **`$LogFile` Journal Initialization & Non-Overlapping Layout**: Initialized `$LogFile` clusters with standard `0xFF` bytes (empty journal state) and corrected LCN layout (`logfile_lcn = mft_mirr_lcn + mirr_clusters`) to prevent `$MFTMirr` from overlapping and corrupting the beginning of `$LogFile`.
    *   **`$LogFile` Duplicate `$DATA` Attribute**: Eliminated redundant `$DATA` attribute creation in `write_mft_record_logfile`.
    *   **MFT Sequence Number Alignment**: Synchronized sequence numbers (`(record_number as u16).max(1)`) across records 0..11 and `sys_ref` references.

### Added
*   **NTFS Boot Mirror & MST/USA Integrity Verification (`NtfsChecker`)**:
    *   `BOOT.PRIMARY`, `BOOT.BACKUP`, `BOOT.MIRROR`: Validates primary VBR, backup VBR mirroring, and geometry agreement.
    *   `MFT.USA` & `MFT.USN`: Validates Multi-Sector Transfer (MST) / Update Sequence Array (USA) invariants and USN sector trailer integrity on MFT records.
    *   Added comprehensive regression and tripwire unit tests across all new check rules.

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
    *   `rimgen`: Refactored into a pure library-first declarative engine with direct stream synthesis (`build_on_io`) and typed event notifications (`BuildEvent`).
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
