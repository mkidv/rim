# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Project Overview

**rim** is a Rust-based filesystem implementation and disk image generation toolkit that supports multiple filesystem types (FAT32, exFAT, EXT4) and can create bootable disk images. The project is designed to work across different environments (std, no_std, UEFI) and provides both library components and command-line tools.

## Common Commands

### Building
```powershell
# Build all workspace members
cargo build

# Build in release mode  
cargo build --release

# Build specific binary (rimgen CLI tool)
cargo build --bin rimgen

# Build with specific features
cargo build --features "std,mem"
```

### Testing
```powershell
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p rimfs
cargo test -p rimpart  
cargo test -p rimio

# Run integration tests (includes disk image comparison tests)
cargo test --test img_diff

# Run benchmarks
cargo bench -p rimfs
```

### Running the CLI Tool
```powershell
# Build a disk image from layout configuration
cargo run --bin rimgen -- build -l layout/layout.toml -o test.img

# Build and create VHD format
cargo run --bin rimgen -- build -l layout/layout.toml -o test.vhd

# Flash to physical device (use with extreme caution)
cargo run --bin rimgen -- flash -l layout/layout.toml -d \\.\PhysicalDrive1 --dry-run
```

### Development and Debugging
```powershell
# Test single filesystem component
cargo test -p rimfs fat32

# Run with debug output
RUST_LOG=debug cargo run --bin rimgen -- build -l layout/layout.toml -o test.img

# Format and check code
cargo fmt
cargo clippy

# Generate documentation
cargo doc --open
```

## Architecture Overview

### Core Design: Trait-Based Filesystem Plugin System

**rim** uses an elegant trait-based architecture that abstracts filesystem operations through a unified plugin system. This allows adding new filesystem support by simply implementing a set of core traits.

### Core Abstraction Layer (`rimfs/src/core/`)

The heart of the architecture is the **`FsFilesystem<'a>` trait** that defines what every filesystem must implement:

```rust
type Meta: FsMeta<Self::AllocUnit>     // Static metadata (sizes, offsets)
type AllocUnit: PartialOrd + Copy     // Logical units (cluster ID, inode number)
type Handle: FsHandle                  // Allocation handle with metadata
type Allocator: FsAllocator<Handle>    // Space allocation management
type Formatter: FsFormatter            // Initial FS structure creation
type Injector: FsNodeInjector<Handle>  // File/directory injection
type Checker: FsChecker                // Structure validation
type Parser: FsParser                  // FS reading/parsing
```

**Key Core Traits:**
- **`FsMeta<Unit>`**: Filesystem metadata (unit sizes, offsets, boundaries)
- **`FsAllocator<Handle>`**: Manages allocation of logical units (clusters, inodes)
- **`FsFormatter`**: Handles initial filesystem formatting
- **`FsNodeInjector<Handle>`**: Recursive file/directory injection
- **`FsChecker`**: Internal structure validation
- **`FsParser`**: Filesystem reading and tree parsing

### Concrete Implementations (`rimfs/src/fs/`)

Each filesystem (FAT32, exFAT, EXT4) provides complete implementations:
- **exFAT**: `ExFatMeta`, `ExFatAllocator`, `ExFatFormatter`, `ExFatInjector`, etc.
- **FAT32**: `Fat32Meta`, `Fat32Allocator`, `Fat32Formatter`, etc.
- **EXT4**: (in development)

This creates a **type-safe plugin system** where each filesystem has its own `AllocUnit` type (cluster IDs vs inode numbers) while sharing the same interface.

### Supporting Components

**rimio** - Block I/O abstraction layer
- Provides `BlockIO` trait for unified disk/memory access
- Multiple backends: `MemBlockIO`, `StdBlockIO`, `UefiBlockIO`
- Streaming operations, chunked I/O, and struct serialization
- Key traits: `BlockIO`, `BlockIOExt`, `BlockIOStreamExt`, `BlockIOStructExt`

**rimpart** - Partition management
- MBR and GPT partition table creation/parsing
- GUID utilities for partition type detection

**rimgen-layout** - Configuration parsing
- TOML layout file parsing for partition definitions
- Size calculations and validation

**rimgen-output** - Output format handling  
- Multiple formats: raw IMG, VHD
- Disk geometry calculations

**rimgen** - CLI orchestration
- Coordinates all components to build disk images
- Handles layout → partition → filesystem → injection pipeline

### Architectural Benefits

**Polymorphism**: Code can work with `dyn FsFilesystem` without knowing concrete types
**Extensibility**: New filesystems require only trait implementations
**Type Safety**: Each filesystem has typed allocation units (prevents mixing cluster IDs with inode numbers)
**Testability**: Each component can be tested independently
**Feature-based compilation**: Support for std/no_std/UEFI environments
**Zero-copy operations**: Uses `zerocopy` crate for safe binary struct handling

## Testing Strategy

### Unit Tests
- Each crate has comprehensive unit tests in `tests/` directories
- Focus on individual component functionality

### Integration Tests  
- `tests/img_diff.rs` - Compares generated images with reference Windows-created images
- Validates filesystem correctness by comparing VBR, FAT, bitmap, and directory structures
- Tests both in-memory and file-based generation

### Benchmarks
- Performance tests in `rimfs/benches/` for filesystem operations
- Critical for optimizing cluster allocation and I/O patterns

### Development Testing
```powershell
# Generate test image and compare with PowerShell analysis scripts
cargo run --bin rimgen -- build -l layout/layout.toml -o test.img
.\exfat_hex_dump.ps1  # Analyze exFAT structures  
.\ext4_hex_dump.ps1   # Analyze EXT4 structures
```

## Working with Filesystems

### Adding New Filesystem Support
1. Create new module in `rimfs/src/fs/[fsname]/`
2. Implement required traits: `Meta`, `Formatter`, `Allocator`, `Injector`, `Parser`
3. Add filesystem-specific structs using zerocopy in `fs_structs_zerocopy.rs`
4. Update `rimfs/src/lib.rs` to expose the new filesystem
5. Add integration tests comparing with reference images

### Layout Configuration
Edit `rimgen/layout/layout.toml`:
```toml
[[partitions]]
name = "Data Partition"
mountpoint = "data/*"  # Files from this directory will be injected
size = "300M"          # or "auto" for remaining space
fs = "exfat"           # supported: fat32, exfat, ext4, raw
```

### File Injection  
Files placed in the `mountpoint` directory (e.g., `layout/data/`) are automatically injected into the filesystem during image creation.

## Development Environment Notes

**Windows-specific**: The project includes PowerShell scripts for analyzing generated disk images and comparing filesystem structures. These are essential for validation during development.

**Cross-platform**: While developed on Windows, the core libraries are designed to work on Linux and macOS through feature flags and conditional compilation.

**No-std support**: Most components work in no_std environments (embedded, UEFI) by using the `alloc` feature instead of `std`.