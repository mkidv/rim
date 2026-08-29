// SPDX-License-Identifier: MIT

use crate::ui::badge::fs_badge;
use crate::ui::format::pretty_bytes;
use colored::Colorize;
use rimgen::LayoutConfig;
use rimpart::scanner::DiskInfo;

/// Print a formatted table representing the declarative layout config.
pub fn print_layout_table(layout: &LayoutConfig) {
    println!(
        "{: <4} | {: <16} | {: <8} | {: <10} | {: <6} | {: <16}",
        "#".bold(),
        "Name".bold(),
        "FS".bold(),
        "Size".bold(),
        "Boot".bold(),
        "Label / Mount".bold()
    );
    println!("{}", "─".repeat(72).dimmed());

    for (i, part) in layout.partitions.iter().enumerate() {
        let size_str = match &part.size {
            rimgen::Size::Fixed(mb) => format!("{} MB", mb),
            rimgen::Size::Auto => "Auto".to_string(),
        };
        let boot_str = if part.bootable {
            "Yes".green().to_string()
        } else {
            "-".dimmed().to_string()
        };
        let label_mount = part
            .label
            .as_deref()
            .or(part.mountpoint.as_deref())
            .unwrap_or("-");

        println!(
            "{: <4} | {: <16} | {: <8} | {: <10} | {: <6} | {: <16}",
            i + 1,
            part.name.bold(),
            fs_badge(&part.fs),
            size_str,
            boot_str,
            label_mount
        );
    }
    println!();
}

/// Print a formatted table of scanned on-disk partitions.
pub fn print_scan_table(scan: &DiskInfo) {
    println!(
        "{: <4} | {: <16} | {: <12} | {: <12} | {: <10}",
        "#".bold(),
        "Name".bold(),
        "Start LBA".bold(),
        "End LBA".bold(),
        "Size".bold()
    );
    println!("{}", "─".repeat(60).dimmed());

    for (i, part) in scan.partitions.iter().enumerate() {
        println!(
            "{: <4} | {: <16} | {: <12} | {: <12} | {: <10}",
            i + 1,
            if part.name.is_empty() {
                "(unnamed)".dimmed().to_string()
            } else {
                part.name.bold().to_string()
            },
            part.start_lba,
            part.end_lba,
            pretty_bytes(part.size_bytes)
        );
    }
    println!();
}
