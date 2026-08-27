# rimfs-exfat

`rimfs-exfat` is the dedicated ExFAT filesystem driver of the RIM ecosystem, supporting large files (>4 GiB) and flash storage devices.

## Features

- **Standard ExFAT Compliance**: Volume Boot Record (VBR), Main and Backup boot sectors with checksum verification.
- **Allocation Bitmap**: Bit-level cluster allocation tracking for contiguous and fragmented files.
- **Upcase Table**: Embedded uppercase conversion table for case-insensitive filename comparison.
- **Directory Entries**: Stream extension, file name entries (up to 255 UTF-16 characters), and volume label records.
- **Pipeline Components**:
  - `ExFatFormatter`: Formats ExFAT volumes with proper alignment and boot parameters.
  - `ExFatAllocator`: Manages cluster allocations via the volume allocation bitmap.
  - `ExFatInjector`: Injects files and directory trees into the volume.
  - `ExFatResolver`: Resolves paths, attributes, and file stream clusters.
  - `ExFatChecker`: Validates VBR checksums, bitmap integrity, and directory entry chains.

## Usage

```toml
[dependencies]
rimfs-exfat = { version = "0.6.3", default-features = false, features = ["std"] }
```

```rust
use rimfs_exfat::prelude::*;
use rimio::StdRimIO;
use std::fs::File;

let mut file = File::options().read(true).write(true).open("exfat.img")?;
let mut disk = StdRimIO::new(&mut file);
let meta = ExFatMeta::new(disk.len(), Some("FLASH_DRIVE"));

ExFatFormatter::new(&mut disk, &meta).format()?;
```

