# rimgen

`rimgen` is a library-first declarative disk storage synthesis engine for the **RIM** ecosystem.

It automates the pipeline of partitioning, formatting, and file injection from a declarative layout description (`DiskLayout`).

## Key Features

- **Pure Rust & Rootless**: Runs in userspace without requiring root privileges or loop device mounts.
- **Direct Stream Synthesis (`build_on_io`)**: Build directly onto any `RimIO` stream (RAM buffers, raw files, UEFI blocks) without writing temporary files.
- **Typed Event Notifications (`BuildEvent`)**: Real-time event hooks for layout planning, GPT writes, partition formatting, and payload progress.
- **Container Format Wrapping**: Native integration with `rimimg` for automatic packaging into VHD, VMDK, QCOW2, and VDI formats.

## Basic Usage

```rust
use rimgen::{DiskLayout, ImageBuilder};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let layout = DiskLayout::from_file(Path::new("layout.toml"))?;

    let mut builder = ImageBuilder::new(layout)
        .truncate(true)
        .on_event(|event| {
            println!("Event: {:?}", event);
        });

    let report = builder.build_to_file("output.img")?;
    println!("Built {} bytes in {:?}", report.total_bytes, report.total_duration);

    Ok(())
}
```

## License

MIT License.
