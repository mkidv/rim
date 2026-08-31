# rimgen

`rimgen` is a library-first declarative disk storage synthesis engine for the **RIM** ecosystem.

It automates the pipeline of partitioning, formatting, and file injection from a declarative layout description (`DiskLayout`).

## Key Features

- **Pure Rust & Rootless**: Runs in userspace without requiring root privileges or loop device mounts.
- **Direct Stream Synthesis (`build_on_io`)**: Build directly onto any `RimIO` stream (RAM buffers, raw files, UEFI blocks) without writing temporary files.
- **Typed Event Notifications (`BuildEvent`)**: Real-time event hooks for layout planning, GPT writes, partition formatting, and payload progress.
- **Direct Container Output**: Native integration with `rimimg` container-backed I/O for VHD, VMDK, QCOW2, and VDI outputs.

## Basic Usage

```rust
use rimgen::{build_config_on_io, LayoutConfig};
use rimio::prelude::MemRimIO;

fn main() -> anyhow::Result<()> {
    let layout = LayoutConfig::from_file(std::path::Path::new("layout.toml"))?;
    let raw_len = rimgen::builder::gpt::calculate_total_disk_sectors_from_config(&layout) * 512;

    let mut buffer = vec![0u8; raw_len as usize];
    let mut io = MemRimIO::new(&mut buffer);
    let report = build_config_on_io(&layout, &mut io)?;
    println!("Built {} bytes in {:?}", report.total_bytes, report.total_duration);

    Ok(())
}
```

## License

MIT License.

## Release Notes

Release notes are tracked in the workspace [CHANGELOG](https://github.com/mkidv/rim/blob/main/CHANGELOG.md).
