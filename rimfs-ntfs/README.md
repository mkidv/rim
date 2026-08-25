# rimfs-ntfs

`rimfs-ntfs` is a pure-Rust, zero-dependency implementation of the **NTFS 3.1** (Windows NT / 2000 / XP / 7 / 10 / 11) filesystem.

## Features

- **Master File Table (MFT)**: Parses and constructs standard 1024-byte `FILE` records, update sequence arrays (USA fixups), and record attributes.
- **Attributes**:
  - Resident and Non-Resident attributes.
  - `$STANDARD_INFORMATION` (timestamps, DOS file permissions).
  - `$FILE_NAME` (Win32, DOS, and POSIX namespaces).
  - `$DATA` (default unnamed streams and named alternate data streams).
  - Data run compression and fragmented cluster run encoding/decoding.
- **B-Tree Directory Indexing**: Index Root (`$INDEX_ROOT`), Index Allocation (`$INDEX_ALLOCATION`), and `$BITMAP` for large directory performance with `INDX` records.
- **Security Descriptors**: Comprehensive Security ID database management (`$Secure` with `$SDS`, `$SDH`, and `$SII` streams).
- **System Metadata**: `$Boot`, `$MFT`, `$MFTMirr`, `$LogFile`, `$Volume`, `$AttrDef`, `$Bitmap`, `$BadClust`, and `$UpCase`.
- **Pipeline Components**:
  - `NtfsFormatter`: Initializes boot sectors, system MFT records, and security streams.
  - `NtfsAllocator`: Manages cluster allocations via the volume cluster bitmap.
  - `NtfsInjector`: Injects files, directories, and data streams into the MFT.
  - `NtfsResolver`: MFT traversal, directory B-tree lookups, and attribute parsing.
  - `NtfsChecker`: Verifies boot sector parameters, MFT signatures, USA fixups, and system record integrity.

## Usage

```toml
[dependencies]
rimfs-ntfs = { version = "0.6.0", default-features = false, features = ["std"] }
```

```rust
use rimfs_ntfs::prelude::*;
use rimio::StdRimIO;
use std::fs::File;

let mut file = File::options().read(true).write(true).open("ntfs.img")?;
let mut disk = StdRimIO::new(&mut file);
let meta = NtfsMeta::new(disk.len(), Some("WINDOWS"));

NtfsFormatter::new(&mut disk, &meta).format()?;
```

