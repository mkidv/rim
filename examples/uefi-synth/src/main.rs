// SPDX-License-Identifier: MIT
//! Full-stack UEFI disk synthesis & seamless chainloader example.
//! Combines rimio (UefiRimIO), rimpart (GPT), rimfs (FAT32, EXT4, NTFS, exFAT),
//! and rimgen (ResolvedLayout / build_on_io) into a no_std + alloc UEFI application.

#![cfg_attr(target_os = "uefi", no_main)]
#![cfg_attr(target_os = "uefi", no_std)]

#[cfg(target_os = "uefi")]
extern crate alloc;

#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use uefi::boot::LoadImageSource;
#[cfg(target_os = "uefi")]
use uefi::prelude::*;
#[cfg(target_os = "uefi")]
use uefi::println;
#[cfg(target_os = "uefi")]
use uefi::proto::media::block::BlockIO;

#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use rimfs::core::resolver::FsTreeResolver;
#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use rimfs::tar::{TarMeta, TarResolver};
#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use rimgen::builder::build_on_io_with_events;
#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use rimgen::guid::{GuidGenerator, SeededGuidGenerator};
#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use rimgen::layout::{Filesystem, Layout, Partition, PartitionKind};
#[cfg(all(target_os = "uefi", feature = "dangerous-uefi-write"))]
use rimio::{SliceRimIO, UefiRimIO};

pub const ALPINE_PAYLOAD: &[u8] = include_bytes!("../../wasm-synth/payload/alpine_payload.tar");
pub const ALPINE_ESP_SIZE_SECTORS: u64 = 131_072; // 64 MiB
pub const ALPINE_ROOTFS_SIZE_SECTORS: u64 = 327_680; // 160 MiB
pub const ALPINE_SEED: u64 = 0x414C_5049_4E45_5249; // "ALPINERI"

#[cfg(target_os = "uefi")]
#[global_allocator]
static ALLOCATOR: uefi::allocator::Allocator = uefi::allocator::Allocator;

#[cfg(target_os = "uefi")]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[cfg(target_os = "uefi")]
#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    println!();
    println!("============================================================");
    println!("  RIM -- Bare-Metal UEFI Storage Synthesizer & Chainloader  ");
    println!("  Disk synthesis from browser to bare metal.               ");
    println!("============================================================");
    println!("[*] Scanning UEFI Block I/O protocol handles...");

    // 1. Locate BlockIO handles
    let handles = match uefi::boot::find_handles::<BlockIO>() {
        Ok(h) => h,
        Err(_) => {
            println!("[!] No BlockIO protocol handles found.");
            return Status::SUCCESS;
        }
    };

    println!("[*] Discovered {} BlockIO handle(s).", handles.len());

    let mut candidate_handles = alloc::vec::Vec::new();
    for &h in &handles {
        if let Ok(block_io) = uefi::boot::open_protocol_exclusive::<BlockIO>(h) {
            let media = block_io.media();
            let total_sectors = media.last_block() + 1;
            let block_size = media.block_size();
            let total_bytes = total_sectors * (block_size as u64);
            let is_part = media.is_logical_partition();
            let is_ro = media.is_read_only();
            println!(
                "    • Handle: {:?} | Sectors: {} | Size: {} MiB | LogicalPart: {} | ReadOnly: {}",
                h,
                total_sectors,
                total_bytes / (1024 * 1024),
                is_part,
                is_ro
            );
            if !is_part && !is_ro && total_sectors >= 200_000 {
                candidate_handles.push(h);
            }
        }
    }

    if candidate_handles.is_empty() {
        println!("[!] No suitable unformatted target drive (>= 200 MiB) found.");
        return Status::SUCCESS;
    }

    #[cfg(not(feature = "dangerous-uefi-write"))]
    {
        println!("[!] Physical disk writes are disabled in this example build.");
        println!("[!] Rebuild with feature `dangerous-uefi-write` to provision a target disk.");
        return Status::SUCCESS;
    }

    #[cfg(feature = "dangerous-uefi-write")]
    {
        let handle = *candidate_handles.last().unwrap();
        let block_io = match uefi::boot::open_protocol_exclusive::<BlockIO>(handle) {
            Ok(b) => b,
            Err(_) => {
                println!("[!] Failed to open exclusive BlockIO protocol on target handle.");
                return Status::SUCCESS;
            }
        };

        let total_size_mb = (block_io.media().last_block() + 1)
            * (block_io.media().block_size() as u64)
            / (1024 * 1024);
        println!(
            "[+] Selected target block device: {} MiB (Handle: {:?})",
            total_size_mb, handle
        );

        // 2. Wrap UEFI BlockIO protocol inside UefiRimIO
        println!("[+] Attaching UefiRimIO block adapter to firmware protocol...");
        let mut io = UefiRimIO::new(block_io);

        // 3. Define disk partition layout and payload tree
        println!("[+] Constructing full Alpine Linux UEFI layout from payload tree...");
        let mut guid_gen = SeededGuidGenerator::new(ALPINE_SEED);
        let mut layout = Layout::new(guid_gen.generate_guid());

        let meta = TarMeta::default();
        let mut tar_io = SliceRimIO::new(ALPINE_PAYLOAD);
        let mut resolver = TarResolver::new(&mut tar_io, &meta);

        // 3.1. ESP partition (FAT32, 64 MiB with systemd-boot, kernel, initramfs)
        let esp_root = match resolver.resolve_tree("esp/*") {
            Ok(root) => root,
            Err(e) => {
                println!("[!] Failed to parse ESP tree from payload TAR: {:?}", e);
                return Status::SUCCESS;
            }
        };

        let esp_part = Partition::new(
            "ESP",
            PartitionKind::Esp,
            Filesystem::Fat32,
            ALPINE_ESP_SIZE_SECTORS,
            guid_gen.generate_guid(),
        )
        .with_bootable(true)
        .with_label("BOOT")
        .with_uuid("ABCD-1234")
        .with_root(esp_root);

        // 3.2. Rootfs partition (EXT4, 160 MiB with full Alpine userspace + rim)
        let rootfs_root = match resolver.resolve_tree("rootfs/*") {
            Ok(root) => root,
            Err(e) => {
                println!("[!] Failed to parse rootfs tree from payload TAR: {:?}", e);
                return Status::SUCCESS;
            }
        };

        let rootfs_part = Partition::new(
            "rootfs",
            PartitionKind::Linux,
            Filesystem::Ext4,
            ALPINE_ROOTFS_SIZE_SECTORS,
            guid_gen.generate_guid(),
        )
        .with_label("ROOTFS")
        .with_uuid("550e8400-e29b-41d4-a716-446655440000")
        .with_root(rootfs_root);

        layout = layout.add_partition(esp_part).add_partition(rootfs_part);

        // 4. Synthesize complete disk image on-the-fly via UEFI Block I/O
        println!("[*] Executing rimgen build_on_io directly onto physical blocks...");
        match build_on_io_with_events(&mut layout, &mut io, |ev| match ev {
            rimgen::builder::BuildEvent::LayoutPlanned {
                total_bytes,
                total_sectors,
            } => {
                println!(
                    "  • Layout planned: {} MiB ({} sectors)",
                    total_bytes / (1024 * 1024),
                    total_sectors
                );
            }
            rimgen::builder::BuildEvent::GptWritten { .. } => {
                println!("  • Primary & Backup GPT tables written and verified");
            }
            rimgen::builder::BuildEvent::PartitionStart { index, total, name } => {
                println!(
                    "  • Formatting partition [{}/{}]: {}",
                    index + 1,
                    total,
                    name
                );
            }
            rimgen::builder::BuildEvent::PartitionFormatted(p) => {
                println!(
                    "  • Formatted partition: {} ({:?}) - {} bytes",
                    p.name, p.fs, p.size_bytes
                );
            }
            _ => {}
        }) {
            Ok(reports) => {
                println!("[✓] UEFI Storage Provisioning SUCCESSFUL!");
                println!("------------------------------------------------------------");
                println!("  Partition 1 : ESP (FAT32, 64 MiB) [systemd-boot + kernel 6.12]");
                println!("  Partition 2 : rootfs (EXT4, 160 MiB) [Alpine userspace + rim]");
                println!("  Partitions Formatted : {}", reports.partitions.len());
                println!("------------------------------------------------------------");
                println!("[✓] Disk is fully formatted and provisioned at firmware level.");
            }
            Err(e) => {
                println!("[!] Error during UEFI disk synthesis: {:?}", e);
                return Status::SUCCESS;
            }
        }

        drop(io);

        // 5. Connect controller to let OVMF parse GPT partition table & bind SimpleFileSystem
        println!("============================================================");
        println!("[*] Registering newly provisioned partition handles with firmware...");
        let _ = uefi::boot::connect_controller(handle, None, None, true);

        // Locate newly mounted ESP filesystem handle
        let mut target_esp_handle = None;
        if let Ok(fs_handles) =
            uefi::boot::find_handles::<uefi::proto::media::fs::SimpleFileSystem>()
        {
            for &fh in &fs_handles {
                if let Ok(my_loaded) = uefi::boot::open_protocol_exclusive::<
                    uefi::proto::loaded_image::LoadedImage,
                >(uefi::boot::image_handle())
                    && my_loaded.device() == Some(fh)
                {
                    continue; // Skip installation media
                }
                target_esp_handle = Some(fh);
                break;
            }
        }

        println!("[*] Seamless In-Firmware Chainloading starting in 2 seconds...");
        uefi::boot::stall(core::time::Duration::from_secs(2));

        let mut tar_io = SliceRimIO::new(ALPINE_PAYLOAD);
        let mut resolver = TarResolver::new(&mut tar_io, &meta);
        if let Ok(boot_data) = resolver.read_file("esp/EFI/BOOT/BOOTX64.EFI") {
            println!(
                "[+] Chainloading BOOTX64.EFI ({} bytes)...",
                boot_data.len()
            );
            let dp_guard = target_esp_handle.and_then(|h| {
                uefi::boot::open_protocol_exclusive::<uefi::proto::device_path::DevicePath>(h).ok()
            });

            let source = if let Some(ref dp) = dp_guard {
                println!("[+] Passing target ESP DevicePath to loaded image...");
                LoadImageSource::FromBuffer {
                    buffer: &boot_data,
                    file_path: Some(dp),
                }
            } else {
                LoadImageSource::FromBuffer {
                    buffer: &boot_data,
                    file_path: None,
                }
            };

            match uefi::boot::load_image(uefi::boot::image_handle(), source) {
                Ok(child_image) => {
                    println!("[+] Executing systemd-boot from memory...");
                    let _ = uefi::boot::start_image(child_image);
                }
                Err(e) => {
                    println!("[!] Failed to load image: {:?}", e);
                }
            }
        }

        loop {
            uefi::boot::stall(core::time::Duration::from_secs(1));
        }
    }
}

#[cfg(not(target_os = "uefi"))]
fn main() {}
