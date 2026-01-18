use rimio::prelude::*;
use rimpart::{gpt, mbr, DEFAULT_SECTOR_SIZE};
use std::fs::File;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} <vhd_file> [--sector N]", args[0]);
        return;
    }

    let vhd_path = &args[1];
    println!("Analyzing VHD: {}", vhd_path);

    let mut file = File::open(vhd_path).expect("Failed to open VHD file");
    let mut io = StdRimIO::new(&mut file);

    // First, analyze MBR and GPT structure using rimpart
    let partition_offset = analyze_disk_structure(&mut io);

    println!(
        "\nexFAT partition starts at offset: 0x{:X}",
        partition_offset
    );

    // Now analyze the exFAT boot sectors at the correct partition offset
    for sector in 0..12 {
        let sector_offset = partition_offset + (sector * DEFAULT_SECTOR_SIZE);
        let mut buffer = [0u8; 512];
        io.read_at(sector_offset, &mut buffer)
            .expect("Failed to read sector");

        println!("\n=== SECTOR {} ===", sector);

        // Print first bytes (more for sector 0)
        let bytes_to_show = if sector == 0 { 120 } else { 64 };
        for i in (0..bytes_to_show).step_by(16) {
            print!("{:04X}: ", i);
            for j in 0..16 {
                if i + j < bytes_to_show {
                    print!("{:02X} ", buffer[i + j]);
                }
            }
            print!(" | ");
            for j in 0..16 {
                if i + j < bytes_to_show {
                    let c = buffer[i + j];
                    if c >= 32 && c <= 126 {
                        print!("{}", c as char);
                    } else {
                        print!(".");
                    }
                }
            }
            println!();
        }

        // Check boot signature
        let signature = u16::from_le_bytes([buffer[510], buffer[511]]);
        if signature == 0xAA55 {
            println!("✓ Valid boot signature (0x55AA)");
        } else {
            println!("✗ Invalid boot signature: 0x{:04X}", signature);
        }

        // Analyze sector 0 in detail
        if sector == 0 {
            println!(
                "Jump Boot: {:02X} {:02X} {:02X}",
                buffer[0], buffer[1], buffer[2]
            );
            let fs_name = String::from_utf8_lossy(&buffer[3..11]);
            println!("FS Name: '{}'", fs_name);

            // Additional exFAT specific fields
            if buffer.len() >= 120 {
                let bytes_per_sector_shift = buffer[108];
                let sectors_per_cluster_shift = buffer[109];
                let bytes_per_sector = 1u16 << bytes_per_sector_shift;
                let sectors_per_cluster = 1u8 << sectors_per_cluster_shift;
                println!(
                    "Bytes per sector shift: {} -> {} bytes",
                    bytes_per_sector_shift, bytes_per_sector
                );
                println!(
                    "Sectors per cluster shift: {} -> {} sectors",
                    sectors_per_cluster_shift, sectors_per_cluster
                );

                let partition_offset = u64::from_le_bytes([
                    buffer[64], buffer[65], buffer[66], buffer[67], buffer[68], buffer[69],
                    buffer[70], buffer[71],
                ]);
                let volume_length = u64::from_le_bytes([
                    buffer[72], buffer[73], buffer[74], buffer[75], buffer[76], buffer[77],
                    buffer[78], buffer[79],
                ]);
                let fat_offset =
                    u32::from_le_bytes([buffer[80], buffer[81], buffer[82], buffer[83]]);
                let fat_length =
                    u32::from_le_bytes([buffer[84], buffer[85], buffer[86], buffer[87]]);
                let cluster_heap_offset =
                    u32::from_le_bytes([buffer[88], buffer[89], buffer[90], buffer[91]]);
                let cluster_count =
                    u32::from_le_bytes([buffer[92], buffer[93], buffer[94], buffer[95]]);
                let root_dir_cluster =
                    u32::from_le_bytes([buffer[96], buffer[97], buffer[98], buffer[99]]);
                let volume_serial =
                    u32::from_le_bytes([buffer[100], buffer[101], buffer[102], buffer[103]]);
                let number_of_fats = buffer[110];

                println!("Partition offset: {} sectors", partition_offset);
                println!("Volume length: {} sectors", volume_length);
                println!("FAT offset: {} sectors", fat_offset);
                println!("FAT length: {} sectors", fat_length);
                println!("Cluster heap offset: {} sectors", cluster_heap_offset);
                println!("Cluster count: {}", cluster_count);
                println!("Root directory cluster: {}", root_dir_cluster);
                println!("Volume serial: 0x{:08X}", volume_serial);
                println!("Number of FATs: {}", number_of_fats);
            }
        }
    }

    // Analyze root directory cluster
    analyze_root_cluster(&mut io, partition_offset);
}

fn analyze_root_cluster(io: &mut StdRimIO<File>, partition_offset: u64) {
    println!("\n=== ROOT DIRECTORY CLUSTER ANALYSIS ===");

    // Get actual values from boot sector
    let mut boot_buffer = [0u8; 512];
    io.read_at(partition_offset, &mut boot_buffer)
        .expect("Failed to read boot sector");

    let cluster_heap_offset = u32::from_le_bytes([
        boot_buffer[88],
        boot_buffer[89],
        boot_buffer[90],
        boot_buffer[91],
    ]);
    let root_cluster = u32::from_le_bytes([
        boot_buffer[96],
        boot_buffer[97],
        boot_buffer[98],
        boot_buffer[99],
    ]);
    let sectors_per_cluster_shift = boot_buffer[109];
    let sectors_per_cluster = 1u32 << sectors_per_cluster_shift;

    let root_cluster_sector = cluster_heap_offset + (root_cluster - 2) * sectors_per_cluster;

    println!(
        "Root cluster {} starts at sector: {}",
        root_cluster, root_cluster_sector
    );

    // Show first sector of root cluster with detailed analysis
    let sector_offset = partition_offset + (root_cluster_sector as u64 * DEFAULT_SECTOR_SIZE);
    let mut buffer = [0u8; 512];
    io.read_at(sector_offset, &mut buffer)
        .expect("Failed to read root cluster sector");

    println!("\n=== ROOT CLUSTER SECTOR {} ===", root_cluster_sector);

    // Print entire sector
    for i in (0..512).step_by(16) {
        print!("{:04X}: ", i);
        for j in 0..16 {
            if i + j < 512 {
                print!("{:02X} ", buffer[i + j]);
            }
        }
        print!(" | ");
        for j in 0..16 {
            if i + j < 512 {
                let c = buffer[i + j];
                if c >= 32 && c <= 126 {
                    print!("{}", c as char);
                } else {
                    print!(".");
                }
            }
        }
        println!();
    }
}

fn analyze_disk_structure(io: &mut StdRimIO<File>) -> u64 {
    println!("\n=== DISK STRUCTURE ANALYSIS ===");

    // Parse MBR using rimpart
    match mbr::parse_mbr(io) {
        Ok(mbr_data) => {
            println!(
                "MBR signature: 0x{:04X} ✓",
                u16::from_le_bytes(mbr_data.signature)
            );
            println!("GPT Protective MBR detected ✓");

            // Parse GPT using rimpart
            match gpt::parse_gpt(io) {
                Ok((header, partitions)) => {
                    println!(
                        "GPT header signature: {} ✓",
                        std::str::from_utf8(&header.signature).unwrap()
                    );
                    let partition_entry_lba = header.partition_entry_lba;
                    let num_partition_entries = header.num_partition_entries;
                    let partition_entry_size = header.partition_entry_size;
                    println!("Partition entries LBA: {}", partition_entry_lba);
                    println!("Number of partition entries: {}", num_partition_entries);
                    println!("Partition entry size: {}", partition_entry_size);

                    // Find first non-empty partition
                    if let Some(first_partition) = partitions.first() {
                        let starting_lba = first_partition.starting_lba;
                        let ending_lba = first_partition.ending_lba;
                        let partition_offset_bytes = starting_lba * DEFAULT_SECTOR_SIZE;
                        println!(
                            "First partition: LBA {} - {} (offset 0x{:X})",
                            starting_lba, ending_lba, partition_offset_bytes
                        );
                        return partition_offset_bytes;
                    } else {
                        println!("No partitions found");
                        return 0;
                    }
                }
                Err(e) => {
                    println!("Failed to parse GPT: {:?}", e);
                    return 0;
                }
            }
        }
        Err(e) => {
            println!("Failed to parse MBR: {:?}", e);
            return 0;
        }
    }
}
