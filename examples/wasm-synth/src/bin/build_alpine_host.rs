// SPDX-License-Identifier: MIT
//! Host CLI builder to synthesize the bootable Alpine Linux disk image using the exact same RIM pipeline.

use std::fs;
use std::path::Path;
use std::time::Instant;

use wasm_synth::synthesize_alpine_image;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("==================================================");
    println!(" RIM Bootable Alpine Linux Disk Image Synthesizer ");
    println!("==================================================");

    let payload_tar_path = Path::new("examples/wasm-synth/payload/alpine_payload.tar");
    if !payload_tar_path.exists() {
        eprintln!("Error: Payload TAR not found at {:?}", payload_tar_path);
        eprintln!("Run 'bash examples/wasm-synth/scripts/prepare_alpine.sh' first.");
        std::process::exit(1);
    }

    println!("Loading payload TAR: {:?}", payload_tar_path);
    let tar_bytes = fs::read(payload_tar_path)?;
    println!(
        "Payload TAR loaded: {:.2} MiB",
        tar_bytes.len() as f64 / (1024.0 * 1024.0)
    );

    println!("Starting pure-Rust disk synthesis on MemRimIO...");
    let t0 = Instant::now();
    let (image, report) =
        synthesize_alpine_image(&tar_bytes).map_err(|e| format!("Synthesis failed: {e}"))?;
    let elapsed = t0.elapsed();

    println!(
        "Synthesis completed in {:.2} ms!",
        elapsed.as_secs_f64() * 1000.0
    );
    println!("Report: {report}");

    let out_path = Path::new("target/rim-alpine-boot.img");
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_path, &image)?;
    println!(
        "Wrote bootable disk image to: {:?} ({:.2} MiB)",
        out_path,
        image.len() as f64 / (1024.0 * 1024.0)
    );
    println!("==================================================");
    println!("Ready to boot with native QEMU + OVMF!");
    println!("Command: bash examples/wasm-synth/scripts/boot_qemu.sh target/rim-alpine-boot.img");
    println!("==================================================");

    Ok(())
}
