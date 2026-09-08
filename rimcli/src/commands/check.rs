// SPDX-License-Identifier: MIT

use anyhow::Context;
use colored::Colorize;
use rimfs::{FsChecker, core::checker::ReportDisplayOpts, exfat::*, ext::*, fat::*, ntfs::*};
use rimimg::ImageFormat;
use rimio::RimIO;
use rimio::prelude::StdRimIO;
use std::path::PathBuf;
use tempfile::NamedTempFile;

use crate::commands::convert::{format_from_path, unwrap_file_with_progress};

pub fn run(image: PathBuf, _verbose: u8) -> anyhow::Result<()> {
    println!(
        "{}",
        format!("🔍 Checking disk image: {}", image.display()).bold()
    );

    let format = format_from_path(&image).unwrap_or(ImageFormat::Raw);

    let (_tmp_file, check_path) = if format == ImageFormat::Raw {
        (None, image)
    } else {
        println!("Physical unwrap of {} format...", format);
        let tmp = NamedTempFile::new().context("Failed to create temp file for unwrapping")?;
        let tmp_path = tmp.path().to_path_buf();

        unwrap_file_with_progress(&image, &tmp_path, format, |_, _| {})?;

        (Some(tmp), tmp_path)
    };

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&check_path)?;

    let mut io = StdRimIO::new(&mut file);

    // 1. Scan Disk (GPT / MBR)
    let scan = rimpart::scan_disk_with_sector(&mut io, 512)
        .map_err(|e| anyhow::anyhow!("Failed to scan disk: {}", e))?;

    println!(
        "Sector size: {}  MBR: {:?}  GPT: {}",
        scan.sector_size,
        scan.mbr_kind,
        if scan.gpt_header.is_some() {
            "present".green()
        } else {
            "absent".yellow()
        }
    );

    let mut has_errors = false;

    for (i, p) in scan.partitions.iter().enumerate() {
        println!("#{} {:?} ({}–{})", i, p.name, p.start_lba, p.end_lba);

        let offset = p.start_lba * 512;
        io.set_offset(offset);

        // Try NTFS
        if let Ok(meta) = NtfsMeta::from_io(&mut io) {
            println!("Detected {} on partition {}", "NTFS".cyan(), i);
            let mut checker = NtfsChecker::new(&mut io, &meta);
            match checker.check_all() {
                Ok(report) => {
                    println!(
                        "{}",
                        report.display_with(ReportDisplayOpts {
                            prefix: "    ",
                            ..Default::default()
                        })
                    );
                    if report.has_error() {
                        println!("❌ Partition {} has errors!", i);
                        has_errors = true;
                    } else {
                        println!("✅ Partition {} is clean.", i);
                    }
                }
                Err(e) => {
                    println!("❌ Error running checker: {}", e);
                    has_errors = true;
                }
            }
            continue;
        }

        // Try ExFAT
        if let Ok(meta) = ExFatMeta::from_io(&mut io) {
            println!("Detected {} on partition {}", "ExFAT".cyan(), i);
            let mut checker = ExFatChecker::new(&mut io, &meta);
            match checker.check_all() {
                Ok(report) => {
                    println!(
                        "{}",
                        report.display_with(ReportDisplayOpts {
                            prefix: "    ",
                            ..Default::default()
                        })
                    );
                    if report.has_error() {
                        println!("❌ Partition {} has errors!", i);
                        has_errors = true;
                    } else {
                        println!("✅ Partition {} is clean.", i);
                    }
                }
                Err(e) => {
                    println!("❌ Error running checker: {}", e);
                    has_errors = true;
                }
            }
            continue;
        }

        // Try Ext4
        if let Ok(meta) = ExtMeta::from_io(&mut io) {
            println!("Detected {} on partition {}", "Ext4".cyan(), i);
            let mut checker = ExtChecker::new(&mut io, &meta);
            match checker.check_all() {
                Ok(report) => {
                    println!(
                        "{}",
                        report.display_with(ReportDisplayOpts {
                            prefix: "    ",
                            ..Default::default()
                        })
                    );
                    if report.has_error() {
                        println!("❌ Partition {} has errors!", i);
                        has_errors = true;
                    } else {
                        println!("✅ Partition {} is clean.", i);
                    }
                }
                Err(e) => {
                    println!("❌ Error running checker: {}", e);
                    has_errors = true;
                }
            }
            continue;
        }

        // Try RimFAT/FAT
        if let Ok(meta) = FatMeta::from_io(&mut io) {
            let fs_type = if meta.use_integrity {
                "RimFAT"
            } else {
                "FAT32"
            };
            println!("Detected {} on partition {}", fs_type.cyan(), i);

            let mut checker = FatChecker::new(&mut io, &meta);
            match checker.check_all() {
                Ok(report) => {
                    println!(
                        "{}",
                        report.display_with(ReportDisplayOpts {
                            prefix: "    ",
                            ..Default::default()
                        })
                    );
                    if report.has_error() {
                        println!("❌ Partition {} has errors!", i);
                        has_errors = true;
                    } else {
                        println!("✅ Partition {} is clean.", i);
                    }
                }
                Err(e) => {
                    println!("❌ Error running checker: {}", e);
                    has_errors = true;
                }
            }
            continue;
        }

        println!(
            "Partition {} is not a recognized FAT/RimFAT/ExFAT/Ext4/NTFS volume.",
            i
        );
    }

    if has_errors {
        anyhow::bail!(
            "Filesystem check failed: one or more partitions contain errors or failed to verify"
        );
    }

    Ok(())
}
