use crate::constants::*;
use crate::helpers::{partition_to_gpt_partition_entry, size_to_sectors};
use rimfs::exfat::*;
use rimfs::fat32::*;
use rimgen_layout::*;
use rimpart::types::GptPartitionEntry;
use rimpart::*;
use std::fs::File;
use std::path::Path;
use uuid::Uuid;

/// Entry point — create disk image
pub fn create(layout: &Layout, output: &Path, truncate: &bool) -> anyhow::Result<()> {
    let mut file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)?;

    let mut io = StdBlockIO::new(&mut file);

    let total_sectors = calculate_total_disk_sectors(layout);
    io.set_len(total_sectors * SECTOR_SIZE);

    let disk_guid = *Uuid::new_v4().as_bytes();

    mbr::write_protective_mbr(&mut io, total_sectors);

    validate_mbr(&mut io);

    // Write GPT partitions
    let mut partition_entries = vec![];

    let mut start = ALIGNMENT;

    for part in &layout.partitions {
        let sectors = size_to_sectors(&part.size);
        let end = start + sectors - 1;

        if end >= total_sectors {
            anyhow::bail!(
                "Partition '{}' does not fit in disk image ({} > {})",
                part.name,
                end,
                total_sectors
            );
        }

        partition_entries.push(partition_to_gpt_partition_entry(part, start, end)?);

        start = end + ALIGNMENT;
    }

    gpt::write_gpt(&mut io, &partition_entries, total_sectors, disk_guid).unwrap();

    validate_gpt(&mut io);

    // Optionally truncate
    if *truncate {
        utils::truncate_image(&mut io, &partition_entries, total_sectors);
    }

    // Drop io before mutably borrowing file again
    drop(io);

    // Format + inject content
    format_inject(&mut file, partition_entries, layout).unwrap();

    Ok(())
}

/// Format + inject content into partitions
fn format_inject(file: &mut File, partitions: Vec<GptPartitionEntry>, layout: &Layout) -> FsResult {
    let mut parser = StdFsParser::new();

    for (i, part) in layout.partitions.iter().enumerate() {
        let source_path = layout.base_dir.join(&part.mountpoint);
        let node = parser.parse_tree(source_path.to_str().unwrap())?;
        match part.fs {
            Filesystem::Fat32 => {
                format_inject_fat32(file, partitions[i], part, &node)?;
            }
            Filesystem::ExFat => {
                format_inject_exfat(file, partitions[i], part, &node)?;
            }
            Filesystem::Raw => {
                format_raw(file, partitions[i], part)?;
            }
            // Filesystem::Ext4 => {
            //     format_inject_ext4(&mut file, &gpt, i, layout, part, &mut parser)?;
            // }
            _ => {
                println!(
                    "[rimgen] Skipping partition '{}': unsupported filesystem {:?}",
                    part.name, part.fs
                );
            }
        }
    }

    Ok(())
}

/// Format + inject FAT32 partition
fn format_inject_fat32(
    file: &mut File,
    entry: GptPartitionEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult {
    let starting_lba = entry.starting_lba;
    let ending_lba = entry.ending_lba;
    let offset = starting_lba * SECTOR_SIZE;
    let size_bytes = (ending_lba - starting_lba + 1) * SECTOR_SIZE;

    let mut io = StdBlockIO::new_with_offset(file, offset);

    let meta = Fat32Meta::new(size_bytes, Some(&part.name));

    let mut formatter = Fat32Formatter::new(&mut io, &meta);
    formatter.format(false)?;

    let mut allocator = Fat32Allocator::new(&meta);
    let mut injector = Fat32Injector::new(&mut io, &mut allocator, &meta);
    injector.inject_tree(node)?;

    let mut checker = Fat32Checker::new(&mut io, &meta);
    checker.check_all()?;

    let mut parser = Fat32Parser::new(&mut io, &meta);
    let fs = parser.parse_tree("/*")?;
    println!("[rimgen] On disk :");
    println!("{fs}");

    Ok(())
}

fn format_inject_exfat(
    file: &mut File,
    entry: GptPartitionEntry,
    part: &Partition,
    node: &FsNode,
) -> FsResult {
    let starting_lba = entry.starting_lba;
    let ending_lba = entry.ending_lba;
    let offset = starting_lba * SECTOR_SIZE;
    let size_bytes = (ending_lba - starting_lba + 1) * SECTOR_SIZE;

    let mut io = StdBlockIO::new_with_offset(file, offset);

    println!(
        "[rimgen] Partition '{}': GPT LBA {} (offset {}), size {} bytes",
        part.name, starting_lba, offset, size_bytes
    );

    let meta = ExFatMeta::new(size_bytes, Some(&part.name));

    let mut formatter = ExFatFormatter::new(&mut io, &meta);
    formatter.format(false)?;

    let mut allocator = ExFatAllocator::new(&meta);
    let mut injector = ExFatInjector::new(&mut io, &mut allocator, &meta);
    injector.inject_tree(node)?;

    let mut checker = ExFatChecker::new(&mut io, &meta);
    // checker.check_all()?;

    let mut parser = ExFatParser::new(&mut io, &meta);
    let fs = parser.parse_tree("/*")?;
    println!("[rimgen] On disk :");
    println!("{fs}");

    Ok(())
}

fn format_raw(file: &mut File, entry: GptPartitionEntry, part: &Partition) -> FsResult {
    let starting_lba = entry.starting_lba;
    let ending_lba = entry.ending_lba;
    let offset = starting_lba * SECTOR_SIZE;
    let size_bytes = (ending_lba - starting_lba + 1) * SECTOR_SIZE;

    println!(
        "[rimgen] Partition '{}' (RAW): GPT LBA {} (offset {}), size {} bytes",
        part.name, starting_lba, offset, size_bytes
    );

    let mut io = StdBlockIO::new_with_offset(file, offset);
    io.zero_fill(0, size_bytes as usize)?;

    println!("[rimgen] RAW partition left empty (zero-filled)");
    Ok(())
}

/// Calculate total disk sectors needed
fn calculate_total_disk_sectors(layout: &Layout) -> u64 {
    layout
        .partitions
        .iter()
        .map(|p| size_to_sectors(&p.size) + ALIGNMENT)
        .sum::<u64>()
        + ALIGNMENT
}

/// Validate GPT content (debug)
fn validate_gpt(io: &mut dyn BlockIO) -> anyhow::Result<()> {
    use uuid::Uuid;

    println!("=== GPT Validation ===");

    let (header, partitions) =
        gpt::parse_gpt(io).map_err(|e| anyhow::anyhow!("parse_gpt error: {:?}", e))?;

    // Basic header info
    println!("Primary GPT Header:");
    println!(
        "  Signature: {:?}",
        std::str::from_utf8(&header.signature).unwrap_or("<invalid>")
    );
    let revision = header.revision;
    println!("  Revision: {revision:08X}");
    let header_size = header.header_size;
    let current_lba = header.current_lba;
    let backup_lba = header.backup_lba;
    let first_usable_lba = header.first_usable_lba;
    let last_usable_lba = header.last_usable_lba;
    let partition_entry_lba = header.partition_entry_lba;
    let num_partition_entries = header.num_partition_entries;
    let partition_entry_size = header.partition_entry_size;

    println!("  Header size: {header_size}");
    println!("  Current LBA: {current_lba}");
    println!("  Backup LBA: {backup_lba}");
    println!("  First usable LBA: {first_usable_lba}");
    println!("  Last usable LBA: {last_usable_lba}");
    println!("  Partition entries LBA: {partition_entry_lba}");
    println!("  Num partition entries: {num_partition_entries}");
    println!("  Partition entry size: {partition_entry_size}");
    println!("  Disk GUID: {}", Uuid::from_bytes(header.disk_guid));

    // Display partitions
    println!("Partitions:");

    if partitions.is_empty() {
        println!("  (no valid partitions)");
    }

    for (idx, part) in partitions.iter().enumerate() {
        let type_guid = Uuid::from_bytes(part.partition_type_guid).to_string();
        let unique_guid = Uuid::from_bytes(part.unique_partition_guid).to_string();
        let starting_lba = part.starting_lba;
        let ending_lba = part.ending_lba;
        let size_bytes = (ending_lba - starting_lba + 1) * 512;

        // Decode UTF-16 partition label
        let partition_name: [u16; 36] = part.partition_name;
        let name_utf16: Vec<u16> = partition_name
            .iter()
            .cloned()
            .take_while(|&c| c != 0)
            .collect();
        let label = String::from_utf16(&name_utf16).unwrap_or_else(|_| "<invalid>".to_string());

        // Heuristic → is known Windows Basic Data
        let is_windows_data = guids::is_data_partition(part);
        println!("  [{idx}] LBA {starting_lba} - {ending_lba} ({size_bytes} bytes)");
        println!("    Type GUID: {type_guid} (WindowsBasicData: {is_windows_data})");
        println!("    Unique GUID: {unique_guid}");
        println!("    Label: '{label}'");
    }

    println!("=== GPT Validation DONE ===\n");

    Ok(())
}

pub fn validate_mbr(io: &mut dyn BlockIO) -> anyhow::Result<()> {
    println!("=== MBR Validation ===");

    let mbr = mbr::parse_mbr(io).map_err(|e| anyhow::anyhow!("parse_mbr error: {:?}", e))?;

    if mbr.signature != [0x55, 0xAA] {
        println!(
            "ERROR: Invalid MBR signature: {:02X}{:02X}",
            mbr.signature[1], mbr.signature[0]
        );
        return Ok(());
    } else {
        println!("MBR signature OK: 0x55AA");
    }

    for (i, part) in mbr.partition_entries.iter().enumerate() {
        println!("Partition entry {i}:");
        println!("  Boot indicator: 0x{:02X}", part.boot_indicator);
        println!(
            "  Partition type: 0x{:02X} ({})",
            part.partition_type,
            if part.partition_type == 0xEE {
                "Protective GPT"
            } else {
                "Unknown"
            }
        );
        let starting_lba = part.starting_lba;
        let size_in_lba = part.size_in_lba;
        println!("  Starting LBA: {starting_lba}");
        println!("  Size in LBA: {size_in_lba}");
    }

    println!("=== MBR Validation DONE ===\n");
    Ok(())
}
