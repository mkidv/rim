# uefi-synth

`uefi-synth` is a bare-metal UEFI firmware application demonstrating in-memory storage synthesis, partition table manipulation, and filesystem verification using RIM in `#![no_std]` without the standard library.

## Capabilities

- **Bare-Metal `#![no_std]`**: Operates directly under UEFI firmware using `uefi-rs` and `UefiRimIO`.
- **In-Memory GPT Synthesis**: Generates GPT partition structures and multi-filesystem layouts in RAM.
- **Embedded Verification**: Performs real-time integrity and geometry validation on bare-metal hardware.

## Building

```bash
cargo build -p uefi-synth --target x86_64-unknown-uefi
```

The default build scans BlockIO devices but does not write to physical disks.
To build the provisioning demo that writes the generated layout to a selected target, enable the explicit safety gate:

```bash
cargo build -p uefi-synth --target x86_64-unknown-uefi --features dangerous-uefi-write
```
