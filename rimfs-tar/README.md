# rimfs-tar

`rimfs-tar` is the dedicated POSIX UStar TAR archive driver of the RIM ecosystem, providing `no_std + alloc` streaming creation, extraction, resolution, and validation of TAR archives conforming to the standard `rimfs` filesystem pipeline.

## Features

- **POSIX UStar Compliant**: Standard 512-byte header serialization, octal field encoding, header checksum validation, and end-of-archive marker (`2 x 512` zero blocks).
- **Embedded & Bare-Metal (`no_std + alloc`)**: Fully operational in constrained environments without `std`.
- **Pipeline Components**:
  - `TarFormatter`: Initializes archive header structures.
  - `TarAllocator`: 512-byte block allocation tracking.
  - `TarInjector`: Streaming file, directory, and symlink writer into TAR archives.
  - `TarResolver`: In-place archive traversal, path resolution, and file extraction.
  - `TarChecker`: Consistency validation for headers, octal numbers, checksums, and trailer records.

## Usage

```toml
[dependencies]
rimfs-tar = { version = "0.8.1", default-features = false, features = ["alloc"] }
```

```rust
use rimfs_tar::prelude::*;
use rimio::MemRimIO;

let mut buf = vec![0u8; 10 * 1024 * 1024];
let mut io = MemRimIO::new(&mut buf);
let meta = TarMeta::new(io.len(), Some("ARCHIVE")).unwrap();

// Initialize TAR archive
TarFormatter::new(&mut io, &meta).format(false)?;

// Inject files into archive
let mut injector = TarInjector::new(&mut io, &meta)?;
let mut tree = FsNode::new_dir("/");
injector.inject_tree(&mut tree)?;
injector.flush()?;
```

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
