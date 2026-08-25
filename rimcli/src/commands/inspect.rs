// SPDX-License-Identifier: MIT

use crate::ui::badge::fs_name_badge;
use crate::ui::format::pretty_bytes;
use anyhow::Context;
use colored::Colorize;
use rimimg::ImageFormat;
use rimio::RimIO;
use rimio::prelude::StdRimIO;
use std::fs::File;
use std::path::Path;

pub fn run(image: &Path) -> anyhow::Result<()> {
    let mut file = File::open(image)
        .with_context(|| format!("Failed to open image file: {}", image.display()))?;

    let file_size = file.metadata()?.len();
    let format = ImageFormat::from_file(&mut file)?;

    println!(
        "{}",
        format!("🔍 Image Inspection: {}", image.display()).bold()
    );
    println!(
        "  File Size        : {} ({} bytes)",
        pretty_bytes(file_size).cyan(),
        file_size
    );
    println!("  Container Format : {}", format.to_string().green().bold());

    let mut io = StdRimIO::new(&mut file);
    if let Ok(scan) = rimpart::scan_disk_with_sector(&mut io, 512) {
        println!(
            "  Partition Scheme : {}",
            if scan.gpt_header.is_some() {
                "GPT".green()
            } else {
                format!("{:?}", scan.mbr_kind).yellow()
            }
        );
        println!("  Partition Count  : {}", scan.partitions.len());

        for p in &scan.partitions {
            let offset = p.start_lba * 512;
            io.set_offset(offset);

            let fs_name = if rimfs::ntfs::NtfsMeta::from_io(&mut io).is_ok() {
                "NTFS"
            } else if rimfs::exfat::ExFatMeta::from_io(&mut io).is_ok() {
                "ExFAT"
            } else if rimfs::ext::ExtMeta::from_io(&mut io).is_ok() {
                "Ext4"
            } else if let Ok(meta) = rimfs::fat::FatMeta::from_io(&mut io) {
                if meta.use_integrity {
                    "RimFAT"
                } else {
                    "FAT32"
                }
            } else {
                "Raw / Unknown"
            };

            println!(
                "    • #{}: \"{}\" ({}, {}) → {}",
                p.index,
                p.name.bold(),
                pretty_bytes(p.size_bytes),
                p.kind,
                fs_name_badge(fs_name)
            );
        }
    }

    Ok(())
}
