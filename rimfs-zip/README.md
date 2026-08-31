# rimfs-zip

`rimfs-zip` is the dedicated ZIP archive filesystem driver of the RIM ecosystem, providing `no_std + alloc` streaming creation, extraction, resolution, and verification of ZIP archives conforming to the standard `rimfs` filesystem pipeline.

## Features

- **Standard ZIP & ZIP64 Compliant**: Supports Local File Headers (LFH), Central Directory (CD), End of Central Directory (EOCD), and ZIP64 extensions for archives $> 4\text{ GB}$.
- **Stream-First Writing**: Streams file payloads directly into `RimIO` without caching full files in RAM, calculating CRC32 on-the-fly.
- **Fast $O(1)$ Resolution**: Parses Central Directory into memory for fast random-access file reading and directory inspection.
- **Extended Unix & POSIX Metadata**: Full support for Extended Timestamps (`0x5455`), Info-ZIP Unix UID/GID (`0x7875`), POSIX file modes, directory records, and symlinks.
- **Embedded & Bare-Metal (`no_std + alloc`)**: Fully operational in constrained environments without `std`.
- **Pipeline Components**:
  - `ZipFormatter`: Initializes empty archive structures.
  - `ZipAllocator`: Stream offset allocation tracking.
  - `ZipInjector`: Streaming file, directory, and symlink writer.
  - `ZipResolver`: In-place Central Directory traversal, path resolution, and file extraction.
  - `ZipChecker`: Consistency validation for headers, central directory records, and CRC-32 checksums.

## Usage

```toml
[dependencies]
rimfs-zip = { version = "0.8.0", default-features = false, features = ["alloc"] }
```

```rust
use rimfs_zip::prelude::*;
use rimio::MemRimIO;

let mut buf = vec![0u8; 10 * 1024 * 1024];
let mut io = MemRimIO::new(&mut buf);
let meta = ZipMeta::new(io.len(), Some("ARCHIVE")).unwrap();

// Initialize ZIP archive
ZipFormatter::new(&mut io, &meta).format(false)?;

// Inject files into archive
let mut injector = ZipInjector::new(&mut io, &meta)?;
let mut tree = FsNode::new_dir("/");
injector.inject_tree(&mut tree)?;
injector.flush()?;
```

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
