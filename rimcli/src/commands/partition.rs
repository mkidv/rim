// SPDX-License-Identifier: MIT

use crate::ui::format::pretty_bytes;
use crate::ui::table::print_scan_table;
use anyhow::Context;
use colored::Colorize;
use rimimg::ImageFormat;
use rimio::prelude::StdRimIO;
use std::fs::File;
use std::path::Path;

pub fn run(image: &Path) -> anyhow::Result<()> {
    let mut file = File::open(image)
        .with_context(|| format!("Failed to open image file: {}", image.display()))?;

    let format = ImageFormat::from_file(&mut file)?;

    let mut io = StdRimIO::new(&mut file);
    let scan = rimpart::scan_disk_with_sector(&mut io, 512)
        .map_err(|e| anyhow::anyhow!("Failed to scan partitions: {}", e))?;

    println!(
        "{}",
        format!("📦 Partition Table for: {}", image.display()).bold()
    );
    println!("  Container Format : {}", format.to_string().cyan());
    println!("  Sector Size      : {} bytes", scan.sector_size);
    println!("  MBR Kind         : {:?}", scan.mbr_kind);
    println!(
        "  GPT Status       : {}",
        if scan.gpt_header.is_some() {
            "Present".green()
        } else {
            "Absent".yellow()
        }
    );
    println!("  Partitions       : {}", scan.partitions.len());
    println!();

    if scan.partitions.is_empty() {
        println!("  No partitions found.");
        return Ok(());
    }

    print_scan_table(&scan);

    println!(
        "  {:<4} {:<24} {:<12} {:<12} {:<10} {:<36}",
        "#".bold(),
        "Name".bold(),
        "Start LBA".bold(),
        "End LBA".bold(),
        "Size".bold(),
        "Type GUID".bold()
    );
    println!("  {}", "─".repeat(92).dimmed());

    for p in &scan.partitions {
        let size_str = pretty_bytes(p.size_bytes);
        let guid_str = format_guid(&p.unique_guid);
        println!(
            "  {:<4} {:<24} {:<12} {:<12} {:<10} {:<36}",
            p.index,
            if p.name.is_empty() {
                "(unnamed)"
            } else {
                &p.name
            },
            p.start_lba,
            p.end_lba,
            size_str,
            guid_str
        );
    }

    Ok(())
}

fn format_guid(guid: &[u8; 16]) -> String {
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid[3],
        guid[2],
        guid[1],
        guid[0],
        guid[5],
        guid[4],
        guid[7],
        guid[6],
        guid[8],
        guid[9],
        guid[10],
        guid[11],
        guid[12],
        guid[13],
        guid[14],
        guid[15]
    )
}
