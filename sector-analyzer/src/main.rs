use memchr::memmem;
use rimio::prelude::*;
use rimio::utils::{DiffPrettyOptions, DiffRange, diff_streamed_bytes_pretty};
use rimpart::{DEFAULT_SECTOR_SIZE, gpt, mbr};
use std::fs::File;

mod ntfs;

const CHUNK_SIZE: usize = 1024 * 1024; // 1MB chunks

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage(&args[0]);
        return;
    }

    let command_or_path = &args[1];

    match command_or_path.as_str() {
        "diff" => {
            if args.len() < 4 {
                println!("Usage: {} diff <vhd1> <vhd2> [sectors_count]", args[0]);
                return;
            }
            let sectors_count = args
                .get(4)
                .and_then(|s| s.parse().ok())
                .unwrap_or(10_000u64);
            run_diff(&args[2], &args[3], sectors_count);
        }
        "dump-lba" => {
            if args.len() < 4 {
                println!("Usage: {} dump-lba <vhd> <lba> [count]", args[0]);
                return;
            }
            let vhd_path = &args[2];
            let lba: u64 = args[3].parse().expect("Invalid LBA");
            let count: u32 = args.get(4).and_then(|c| c.parse().ok()).unwrap_or(1);
            let mut file = File::open(vhd_path).expect("Failed to open VHD file");
            let mut io = StdRimIO::new(&mut file);
            dump_sectors(&mut io, lba * DEFAULT_SECTOR_SIZE, count);
        }
        "extract-bin" => {
            if args.len() < 6 {
                println!(
                    "Usage: {} extract-bin <vhd> <lba> <count> <output_file>",
                    args[0]
                );
                return;
            }
            let vhd_path = &args[2];
            let lba: u64 = args[3].parse().expect("Invalid LBA");
            let count: u32 = args[4].parse().expect("Invalid count");
            let output_path = &args[5];
            let mut file = File::open(vhd_path).expect("Failed to open VHD file");
            let mut io = StdRimIO::new(&mut file);
            let mut buffer = vec![0u8; (count * DEFAULT_SECTOR_SIZE as u32) as usize];
            io.read_at(lba * DEFAULT_SECTOR_SIZE, &mut buffer)
                .expect("Failed to read");
            std::fs::write(output_path, buffer).expect("Failed to write output file");
            println!("Extracted {} sectors to {}", count, output_path);
        }
        "dump-ntfs-meta" => {
            if args.len() < 3 {
                println!("Usage: {} dump-ntfs-meta <vhd>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).expect("Failed to open VHD file");
            let mut io = StdRimIO::new(&mut file);
            let off = analyze_disk_structure(&mut io);
            if off > 0 {
                ntfs::analyze_ntfs(&mut io, off);
            }
        }
        "dump-mft" => {
            if args.len() < 4 {
                println!("Usage: {} dump-mft <vhd> <record_number>", args[0]);
                return;
            }
            let record_num: u64 = args[3].parse().expect("Invalid record number");
            let mut file = File::open(&args[2]).expect("Failed to open VHD file");
            let mut io = StdRimIO::new(&mut file);
            let off = analyze_disk_structure(&mut io);
            if off > 0 {
                ntfs::dump_mft_record(&mut io, off, record_num);
            }
        }
        "dump-ntfs-layout" => {
            if args.len() < 3 {
                println!("Usage: {} dump-ntfs-layout <vhd>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).expect("Failed to open VHD file");
            let mut io = StdRimIO::new(&mut file);
            let off = analyze_disk_structure(&mut io);
            if off > 0 {
                ntfs::dump_ntfs_layout(&mut io, off);
            }
        }
        "find" => {
            if args.len() < 4 {
                println!(
                    "Usage: {} find <vhd> <pattern> [start_offset] [limit_bytes]",
                    args[0]
                );
                return;
            }
            let start: u64 = args.get(4).and_then(|s| parse_hex_or_dec(s)).unwrap_or(0);
            let limit: u64 = args
                .get(5)
                .and_then(|s| parse_hex_or_dec(s))
                .unwrap_or(u64::MAX);
            run_find(&args[2], &args[3], start, limit);
        }
        "probe" => {
            if args.len() < 3 {
                println!("Usage: {} probe <vhd>", args[0]);
                return;
            }
            run_probe(&args[2]);
        }
        "entropy" => {
            if args.len() < 3 {
                println!("Usage: {} entropy <vhd> [chunk_size]", args[0]);
                return;
            }
            let chunk_size: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(65536);
            run_entropy(&args[2], chunk_size);
        }
        "check-ntfs" => {
            if args.len() < 3 {
                println!("Usage: {} check-ntfs <vhd>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).expect("Failed to open file");
            let mut io = StdRimIO::new(&mut file);
            let off = analyze_disk_structure(&mut io);
            if off > 0 {
                ntfs::run_check_ntfs(&mut io, off);
            }
        }
        "diff-ntfs-meta" => {
            if args.len() < 4 {
                println!("Usage: {} diff-ntfs-meta <vhd1> <vhd2>", args[0]);
                return;
            }
            let mut f1 = File::open(&args[2]).unwrap();
            let mut f2 = File::open(&args[3]).unwrap();
            let mut io1 = StdRimIO::new(&mut f1);
            let mut io2 = StdRimIO::new(&mut f2);
            let off1 = analyze_disk_structure(&mut io1);
            let off2 = analyze_disk_structure(&mut io2);
            ntfs::run_diff_ntfs_meta(&mut io1, &mut io2, off1, off2);
        }
        "dump-secure" => {
            if args.len() < 3 {
                println!("Usage: {} dump-secure <vhd>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let off = analyze_disk_structure(&mut io);
            if off > 0 {
                ntfs::dump_secure_table(&mut io, off);
            }
        }
        vhd_path => {
            println!("Analyzing disk: {}", vhd_path);
            let mut file = File::open(vhd_path).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let off = analyze_disk_structure(&mut io);
            if off == 0 {
                println!("No valid partition.");
                return;
            }
            let mut b = [0u8; 512];
            if io.read_at(off, &mut b).is_ok() {
                let oem = &b[3..11];
                if oem == b"NTFS    " {
                    ntfs::analyze_ntfs(&mut io, off);
                } else {
                    dump_sectors(&mut io, off, 1);
                }
            }
        }
    }
}

fn print_usage(bin: &str) {
    println!("sector-analyzer - Generic Forensic & Verification Tool");
    println!("Usage: {} <command> [args]", bin);
    println!("\nGeneric Commands:");
    println!("  <vhd_path>                      Quick analysis of the first partition");
    println!("  probe <vhd>                     Scan for known magic signatures");
    println!("  find <vhd> <pattern> [off] [l]  Search for hex/text pattern");
    println!("  entropy <vhd> [chunk]           Analyze data density");
    println!("  diff <vhd1> <vhd2> [sectors]    Sector-by-sector comparison");
    println!("  dump-lba <vhd> <lba> [count]    Hex dump specific sectors");
    println!("  extract-bin <vhd> <lba> <cnt> <out> Extract raw sectors");
    println!("\nNTFS Commands:");
    println!("  dump-ntfs-meta <vhd>            NTFS metadata overview");
    println!(
        "  dump-ntfs-layout <vhd>          Full NTFS system records dump (Reverse Engineering)"
    );
    println!("  dump-mft <vhd> <num>            Dump specific MFT record");
    println!("  check-ntfs <vhd>                Verify NTFS integrity");
    println!("  diff-ntfs-meta <vhd1> <vhd2>    Compare NTFS metadata");
    println!("  dump-secure <vhd>               Detailed dump of $Secure indices");
}

fn parse_hex_or_dec(s: &str) -> Option<u64> {
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).ok()
    } else {
        s.parse().ok()
    }
}

fn run_probe(path: &str) {
    let mut file = File::open(path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut io = StdRimIO::new(&mut file);
    let mut buffer = vec![0u8; CHUNK_SIZE + 512];
    let mut offset = 0u64;
    let sigs: &[(&str, &[u8])] = &[
        ("NTFS", b"NTFS    "),
        ("exFAT", b"EXFAT   "),
        ("FAT32", b"FAT32   "),
        ("GPT Header", b"EFI PART"),
        ("MFT Record", b"FILE"),
        ("Index Record", b"INDX"),
    ];
    while offset < size {
        let to_read = (size - offset).min(CHUNK_SIZE as u64) as usize;
        io.read_at(offset, &mut buffer[..to_read]).ok();
        for (name, sig) in sigs {
            for match_off in memmem::find_iter(&buffer[..to_read], sig) {
                let abs = offset + match_off as u64;
                if abs.is_multiple_of(DEFAULT_SECTOR_SIZE)
                    || (abs % DEFAULT_SECTOR_SIZE == 3 && (*name == "NTFS" || *name == "exFAT"))
                {
                    println!("[+] Found {} at offset 0x{:X}", name, abs);
                }
            }
        }
        offset += to_read as u64 - 512;
        if to_read < CHUNK_SIZE {
            break;
        }
    }
}

fn run_find(path: &str, pattern: &str, start_offset: u64, limit: u64) {
    let mut file = File::open(path).unwrap();
    let mut io = StdRimIO::new(&mut file);
    let pat = if pattern.starts_with("0x") {
        (2..pattern.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&pattern[i..i + 2], 16).unwrap_or(0))
            .collect()
    } else {
        pattern.as_bytes().to_vec()
    };
    let mut buffer = vec![0u8; CHUNK_SIZE + pat.len()];
    let mut offset = start_offset;
    let mut scanned = 0u64;
    while scanned < limit {
        let to_read = (limit - scanned).min(CHUNK_SIZE as u64) as usize;
        if io.read_at(offset, &mut buffer[..to_read]).is_err() {
            break;
        }
        for match_off in memmem::find_iter(&buffer[..to_read], &pat) {
            println!("[!] Match at 0x{:X}", offset + match_off as u64);
        }
        let consumed = to_read.saturating_sub(pat.len() - 1);
        offset += consumed as u64;
        scanned += consumed as u64;
        if to_read < CHUNK_SIZE {
            break;
        }
    }
}

fn run_entropy(path: &str, chunk: usize) {
    let mut file = File::open(path).unwrap();
    let size = file.metadata().unwrap().len();
    let mut io = StdRimIO::new(&mut file);
    let mut buffer = vec![0u8; chunk];
    let mut offset = 0u64;
    while offset < size {
        let to_read = (size - offset).min(chunk as u64) as usize;
        io.read_at(offset, &mut buffer[..to_read]).ok();
        let e = calculate_entropy(&buffer[..to_read]);
        println!("0x{:08X} | {:.2}", offset, e);
        offset += to_read as u64;
    }
}

fn calculate_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let mut e = 0.0;
    let len = data.len() as f64;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / len;
            e -= p * p.log2();
        }
    }
    e
}

fn run_diff(path1: &str, path2: &str, sectors: u64) {
    let mut f1 = File::open(path1).unwrap();
    let mut f2 = File::open(path2).unwrap();
    let mut io1 = StdRimIO::new(&mut f1);
    let mut io2 = StdRimIO::new(&mut f2);
    let off1 = analyze_disk_structure(&mut io1);
    let off2 = analyze_disk_structure(&mut io2);
    for s in 0..sectors {
        let mut b1 = [0u8; 512];
        let mut b2 = [0u8; 512];
        io1.read_at(off1 + s * 512, &mut b1).ok();
        io2.read_at(off2 + s * 512, &mut b2).ok();
        if b1 != b2 {
            println!("\n[!] Diff at Sector {}", s);
            let range = DiffRange::new(off1 + s * 512, off2 + s * 512, 512, 512);
            diff_streamed_bytes_pretty(&mut io1, &mut io2, range, DiffPrettyOptions::new("Diff"))
                .ok();
        }
    }
}

fn dump_sectors(io: &mut dyn RimIO, offset: u64, count: u32) {
    let mut b = vec![0u8; 512];
    for s in 0..count {
        let off = offset + s as u64 * 512;
        io.read_at(off, &mut b).ok();
        println!("\n--- Sector 0x{:X} ---", off);
        for (i, chunk) in b.chunks(16).enumerate() {
            print!("{:04X}: ", i * 16);
            for &byte in chunk {
                print!("{:02X} ", byte);
            }
            print!(" | ");
            for &byte in chunk {
                if (32..=126).contains(&byte) {
                    print!("{}", byte as char);
                } else {
                    print!(".");
                }
            }
            println!();
        }
    }
}

fn analyze_disk_structure(io: &mut StdRimIO<File>) -> u64 {
    if let Ok(mbr) = mbr::read_mbr(io) {
        if let Ok((_h, parts)) = gpt::read_gpt(io)
            && let Some(p) = parts.first()
        {
            return p.start_lba * 512;
        }
        if let Some(p) = mbr
            .aligned_entries()
            .iter()
            .find(|e| e.part_type != 0 && e.part_type != 0xEE)
        {
            return p.start_lba as u64 * 512;
        }
    }
    0
}
