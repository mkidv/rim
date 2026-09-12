// SPDX-License-Identifier: MIT

//! Universal forensic analysis, partition scanning, and filesystem verification tool.

use memchr::memmem;
use rimfs::core::checker::{FsChecker, Severity, VerifyReport};
use rimfs::core::resolver::FsTreeResolver;
use rimio::prelude::*;
use rimio::utils::{DiffPrettyOptions, DiffRange, diff_streamed_bytes_pretty};
use rimpart::DEFAULT_SECTOR_SIZE;
use std::fs::File;

mod detect;
mod ext;
mod fat;
mod ntfs;

use detect::{FsKind, scan_partitions};

const CHUNK_SIZE: usize = 1024 * 1024; // 1MB chunks

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage(&args[0]);
        return;
    }

    let command_or_path = &args[1];

    match command_or_path.as_str() {
        "info" => {
            if args.len() < 3 {
                println!("Usage: {} info <image>", args[0]);
                return;
            }
            run_info(&args[2]);
        }
        "check" => {
            if args.len() < 3 {
                println!("Usage: {} check <image> [partition_index]", args[0]);
                return;
            }
            let part_idx: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            run_check(&args[2], part_idx);
        }
        "ls" => {
            if args.len() < 3 {
                println!("Usage: {} ls <image> [partition_index] [path]", args[0]);
                return;
            }
            let part_idx: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let dir_path = args.get(4).map(|s| s.as_str()).unwrap_or("/");
            run_ls(&args[2], part_idx, dir_path);
        }
        "cat" => {
            if args.len() < 4 {
                println!("Usage: {} cat <image> <file_path> [partition_index]", args[0]);
                return;
            }
            let file_path = &args[3];
            let part_idx: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            run_cat(&args[2], part_idx, file_path);
        }
        "dump-fat-meta" => {
            if args.len() < 3 {
                println!("Usage: {} dump-fat-meta <image> [partition_index]", args[0]);
                return;
            }
            let part_idx: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            run_dump_fat(&args[2], part_idx);
        }
        "dump-ext-meta" => {
            if args.len() < 3 {
                println!("Usage: {} dump-ext-meta <image> [partition_index]", args[0]);
                return;
            }
            let part_idx: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            run_dump_ext(&args[2], part_idx);
        }
        "diff" => {
            if args.len() < 4 {
                println!("Usage: {} diff <img1> <img2> [sectors_count]", args[0]);
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
                println!("Usage: {} dump-lba <image> <lba> [count]", args[0]);
                return;
            }
            let image_path = &args[2];
            let lba: u64 = args[3].parse().expect("Invalid LBA");
            let count: u32 = args.get(4).and_then(|c| c.parse().ok()).unwrap_or(1);
            let mut file = File::open(image_path).expect("Failed to open image file");
            let mut io = StdRimIO::new(&mut file);
            dump_sectors(&mut io, lba * DEFAULT_SECTOR_SIZE, count);
        }
        "extract-bin" => {
            if args.len() < 6 {
                println!(
                    "Usage: {} extract-bin <image> <lba> <count> <output_file>",
                    args[0]
                );
                return;
            }
            let image_path = &args[2];
            let lba: u64 = args[3].parse().expect("Invalid LBA");
            let count: u32 = args[4].parse().expect("Invalid count");
            let output_path = &args[5];
            let mut file = File::open(image_path).expect("Failed to open image file");
            let mut io = StdRimIO::new(&mut file);
            let mut buffer = vec![0u8; (count * DEFAULT_SECTOR_SIZE as u32) as usize];
            io.read_at(lba * DEFAULT_SECTOR_SIZE, &mut buffer)
                .expect("Failed to read");
            std::fs::write(output_path, buffer).expect("Failed to write output file");
            println!("Extracted {} sectors to {}", count, output_path);
        }
        "dump-ntfs-meta" => {
            if args.len() < 3 {
                println!("Usage: {} dump-ntfs-meta <image>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).expect("Failed to open image file");
            let mut io = StdRimIO::new(&mut file);
            let off = find_partition_offset(&mut io);
            ntfs::analyze_ntfs(&mut io, off);
        }
        "dump-mft" => {
            if args.len() < 4 {
                println!("Usage: {} dump-mft <image> <record_number>", args[0]);
                return;
            }
            let record_num: u64 = args[3].parse().expect("Invalid record number");
            let mut file = File::open(&args[2]).expect("Failed to open image file");
            let mut io = StdRimIO::new(&mut file);
            let off = find_partition_offset(&mut io);
            ntfs::dump_mft_record(&mut io, off, record_num);
        }
        "dump-ntfs-layout" => {
            if args.len() < 3 {
                println!("Usage: {} dump-ntfs-layout <image>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).expect("Failed to open image file");
            let mut io = StdRimIO::new(&mut file);
            let off = find_partition_offset(&mut io);
            ntfs::dump_ntfs_layout(&mut io, off);
        }
        "find" => {
            if args.len() < 4 {
                println!(
                    "Usage: {} find <image> <pattern> [start_offset] [limit_bytes]",
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
                println!("Usage: {} probe <image>", args[0]);
                return;
            }
            run_probe(&args[2]);
        }
        "entropy" => {
            if args.len() < 3 {
                println!("Usage: {} entropy <image> [chunk_size]", args[0]);
                return;
            }
            let chunk_size: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(65536);
            run_entropy(&args[2], chunk_size);
        }
        "check-ntfs" => {
            if args.len() < 3 {
                println!("Usage: {} check-ntfs <image>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).expect("Failed to open file");
            let mut io = StdRimIO::new(&mut file);
            let off = find_partition_offset(&mut io);
            ntfs::run_check_ntfs(&mut io, off);
        }
        "diff-ntfs-meta" => {
            if args.len() < 4 {
                println!("Usage: {} diff-ntfs-meta <img1> <img2>", args[0]);
                return;
            }
            let mut f1 = File::open(&args[2]).unwrap();
            let mut f2 = File::open(&args[3]).unwrap();
            let mut io1 = StdRimIO::new(&mut f1);
            let mut io2 = StdRimIO::new(&mut f2);
            let off1 = find_partition_offset(&mut io1);
            let off2 = find_partition_offset(&mut io2);
            ntfs::run_diff_ntfs_meta(&mut io1, &mut io2, off1, off2);
        }
        "dump-secure" => {
            if args.len() < 3 {
                println!("Usage: {} dump-secure <image>", args[0]);
                return;
            }
            let mut file = File::open(&args[2]).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let off = find_partition_offset(&mut io);
            ntfs::dump_secure_table(&mut io, off);
        }
        image_path => {
            run_info(image_path);
        }
    }
}

fn print_usage(bin: &str) {
    println!("sector-analyzer - Universal Forensic & Filesystem Verification Tool");
    println!("Usage: {} <command> [args]", bin);
    println!("\nUniversal Commands:");
    println!("  <image>                         Inspect container, partition scheme, and filesystems");
    println!("  info <image>                    Detailed forensic inspection of container and partitions");
    println!("  check <image> [part_idx]        Run RIM FsChecker on partition (NTFS, FAT, exFAT, EXT4, ISO)");
    println!("  ls <image> [part_idx] [path]    List directory tree via FsTreeResolver without OS mounting");
    println!("  cat <image> <file_path> [part]  Extract and print file contents");
    println!("  probe <image>                   Scan for known magic signatures across the image");
    println!("  find <image> <pattern> [off] [l] Search for hex/text pattern");
    println!("  entropy <image> [chunk]         Analyze data entropy / density map");
    println!("  diff <img1> <img2> [sectors]    Sector-by-sector comparison with pretty diff");
    println!("  dump-lba <image> <lba> [count]  Hex dump specific sectors");
    println!("  extract-bin <image> <lba> <cnt> <out> Extract raw sectors to a binary file");
    println!("\nFormat-Specific Forensic Dumps:");
    println!("  dump-fat-meta <image> [part]    Dump FAT12/16/32 or exFAT BPB, FSInfo and geometry");
    println!("  dump-ext-meta <image> [part]    Dump EXT2/3/4 Superblock and Block Group Descriptors");
    println!("  dump-ntfs-meta <image>          Detailed NTFS MFT and system records overview");
    println!("  dump-mft <image> <record_num>   Dump specific NTFS MFT record with attributes");
    println!("  dump-ntfs-layout <image>        Full NTFS system records layout dump");
    println!("  dump-secure <image>             Detailed dump of NTFS $Secure ($SDS, $SDH, $SII)");
    println!("  check-ntfs <image>              Legacy NTFS integrity check");
}

fn run_info(path: &str) {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            println!("[-] Failed to open '{}': {e}", path);
            return;
        }
    };
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut io = StdRimIO::new(&mut file);

    println!("============================================================");
    println!("  SECTOR-ANALYZER FORENSIC DISK REPORT");
    println!("============================================================");
    println!("File Path:        {}", path);
    println!("File Size:        {} bytes ({:.2} MB)", file_len, file_len as f64 / (1024.0 * 1024.0));

    if let Ok(fmt) = rimimg::ImageFormat::from_io(&mut io) {
        println!("Container Format: {:?}", fmt);
    } else {
        println!("Container Format: Raw Disk / Direct Binary");
    }

    let parts = scan_partitions(&mut io);
    println!("Partitions Found: {}", parts.len());
    println!("------------------------------------------------------------");
    for part in &parts {
        println!("  [Partition #{}]", part.index);
        println!("    Type:         {}", part.part_type);
        println!("    Start Offset: 0x{:X} (Sector {})", part.start_offset, part.start_offset / DEFAULT_SECTOR_SIZE);
        println!("    Size:         {} bytes ({:.2} MB)", part.size_bytes, part.size_bytes as f64 / (1024.0 * 1024.0));
        println!("    Filesystem:   {}", part.fs.name());
        println!();
    }
    println!("============================================================");
}

fn run_check(path: &str, part_idx: usize) {
    let mut file = File::open(path).expect("Failed to open file");
    let mut io = StdRimIO::new(&mut file);
    let parts = scan_partitions(&mut io);
    if parts.is_empty() {
        println!("[-] No partitions detected in {}", path);
        return;
    }
    let part = parts.get(part_idx).unwrap_or_else(|| {
        println!("[-] Partition index {} out of range (found {} partitions), using partition 0", part_idx, parts.len());
        &parts[0]
    });
    println!("[+] Running FsChecker on Partition {} ({}) at offset 0x{:X}", part.index, part.fs.name(), part.start_offset);
    io.set_offset(part.start_offset);

    match part.fs {
        FsKind::Ntfs => {
            match rimfs::ntfs::NtfsMeta::from_io(&mut io) {
                Ok(meta) => {
                    let mut checker = rimfs::ntfs::traits::NtfsChecker::new(&mut io, &meta);
                    match checker.check_all() {
                        Ok(rep) => print_report(&rep),
                        Err(e) => println!("[-] NtfsChecker error: {e:?}"),
                    }
                }
                Err(e) => println!("[-] Failed to parse NtfsMeta: {e:?}"),
            }
        }
        FsKind::Fat => {
            match rimfs::fat::FatMeta::from_io(&mut io) {
                Ok(meta) => {
                    let mut checker = rimfs::fat::traits::FatChecker::new(&mut io, &meta);
                    match checker.check_all() {
                        Ok(rep) => print_report(&rep),
                        Err(e) => println!("[-] FatChecker error: {e:?}"),
                    }
                }
                Err(e) => println!("[-] Failed to parse FatMeta: {e:?}"),
            }
        }
        FsKind::ExFat => {
            match rimfs::exfat::ExFatMeta::from_io(&mut io) {
                Ok(meta) => {
                    let mut checker = rimfs::exfat::traits::ExFatChecker::new(&mut io, &meta);
                    match checker.check_all() {
                        Ok(rep) => print_report(&rep),
                        Err(e) => println!("[-] ExFatChecker error: {e:?}"),
                    }
                }
                Err(e) => println!("[-] Failed to parse ExFatMeta: {e:?}"),
            }
        }
        FsKind::Ext => {
            match rimfs::ext::ExtMeta::from_io(&mut io) {
                Ok(meta) => {
                    let mut checker = rimfs::ext::traits::ExtChecker::new(&mut io, &meta);
                    match checker.check_all() {
                        Ok(rep) => print_report(&rep),
                        Err(e) => println!("[-] ExtChecker error: {e:?}"),
                    }
                }
                Err(e) => println!("[-] Failed to parse ExtMeta: {e:?}"),
            }
        }
        FsKind::Iso => {
            let meta = rimfs::iso::IsoMeta::default();
            let mut checker = rimfs::iso::IsoChecker::new(&mut io, &meta);
            match checker.check_all() {
                Ok(rep) => print_report(&rep),
                Err(e) => println!("[-] IsoChecker error: {e:?}"),
            }
        }
        FsKind::Unknown => {
            println!("[-] Unknown filesystem format at partition offset 0x{:X}", part.start_offset);
        }
    }
}

fn run_ls(path: &str, part_idx: usize, dir_path: &str) {
    let mut file = File::open(path).expect("Failed to open file");
    let mut io = StdRimIO::new(&mut file);
    let parts = scan_partitions(&mut io);
    if parts.is_empty() {
        println!("[-] No partitions detected in {}", path);
        return;
    }
    let part = parts.get(part_idx).unwrap_or(&parts[0]);
    println!("[+] Browsing Partition {} ({}) at path '{}'", part.index, part.fs.name(), dir_path);
    io.set_offset(part.start_offset);

    let entries = match part.fs {
        FsKind::Ntfs => {
            let meta = rimfs::ntfs::NtfsMeta::from_io(&mut io).expect("NtfsMeta failed");
            let mut res = rimfs::ntfs::traits::NtfsResolver::new(&mut io, &meta);
            res.read_dir(dir_path)
        }
        FsKind::Fat => {
            let meta = rimfs::fat::FatMeta::from_io(&mut io).expect("FatMeta failed");
            let mut res = rimfs::fat::traits::FatResolver::new(&mut io, &meta);
            res.read_dir(dir_path)
        }
        FsKind::ExFat => {
            let meta = rimfs::exfat::ExFatMeta::from_io(&mut io).expect("ExFatMeta failed");
            let mut res = rimfs::exfat::traits::ExFatResolver::new(&mut io, &meta);
            res.read_dir(dir_path)
        }
        FsKind::Ext => {
            let meta = rimfs::ext::ExtMeta::from_io(&mut io).expect("ExtMeta failed");
            let mut res = rimfs::ext::traits::ExtResolver::new(&mut io, &meta);
            res.read_dir(dir_path)
        }
        FsKind::Iso => {
            let meta = rimfs::iso::IsoMeta::default();
            let mut res = rimfs::iso::IsoResolver::new(&mut io, &meta);
            res.read_dir(dir_path)
        }
        FsKind::Unknown => {
            println!("[-] Unknown filesystem");
            return;
        }
    };

    match entries {
        Ok(list) => {
            println!("\nDirectory: {}", dir_path);
            if list.is_empty() {
                println!("  (empty)");
            }
            for name in list {
                println!("  {}", name);
            }
        }
        Err(e) => println!("[-] read_dir failed: {e:?}"),
    }
}

fn run_cat(path: &str, part_idx: usize, file_path: &str) {
    let mut file = File::open(path).expect("Failed to open file");
    let mut io = StdRimIO::new(&mut file);
    let parts = scan_partitions(&mut io);
    if parts.is_empty() {
        println!("[-] No partitions detected in {}", path);
        return;
    }
    let part = parts.get(part_idx).unwrap_or(&parts[0]);
    io.set_offset(part.start_offset);

    let bytes = match part.fs {
        FsKind::Ntfs => {
            let meta = rimfs::ntfs::NtfsMeta::from_io(&mut io).expect("NtfsMeta failed");
            let mut res = rimfs::ntfs::traits::NtfsResolver::new(&mut io, &meta);
            res.read_file(file_path)
        }
        FsKind::Fat => {
            let meta = rimfs::fat::FatMeta::from_io(&mut io).expect("FatMeta failed");
            let mut res = rimfs::fat::traits::FatResolver::new(&mut io, &meta);
            res.read_file(file_path)
        }
        FsKind::ExFat => {
            let meta = rimfs::exfat::ExFatMeta::from_io(&mut io).expect("ExFatMeta failed");
            let mut res = rimfs::exfat::traits::ExFatResolver::new(&mut io, &meta);
            res.read_file(file_path)
        }
        FsKind::Ext => {
            let meta = rimfs::ext::ExtMeta::from_io(&mut io).expect("ExtMeta failed");
            let mut res = rimfs::ext::traits::ExtResolver::new(&mut io, &meta);
            res.read_file(file_path)
        }
        FsKind::Iso => {
            let meta = rimfs::iso::IsoMeta::default();
            let mut res = rimfs::iso::IsoResolver::new(&mut io, &meta);
            res.read_file(file_path)
        }
        FsKind::Unknown => {
            println!("[-] Unknown filesystem");
            return;
        }
    };

    match bytes {
        Ok(data) => {
            if let Ok(text) = std::str::from_utf8(&data) {
                print!("{}", text);
            } else {
                println!("[+] Binary file ({} bytes), preview:", data.len());
                for (i, chunk) in data.iter().take(256).enumerate() {
                    if i % 16 == 0 {
                        print!("\n{:04X}: ", i);
                    }
                    print!("{:02X} ", chunk);
                }
                println!();
            }
        }
        Err(e) => println!("[-] read_file failed: {e:?}"),
    }
}

fn run_dump_fat(path: &str, part_idx: usize) {
    let mut file = File::open(path).expect("Failed to open file");
    let mut io = StdRimIO::new(&mut file);
    let parts = scan_partitions(&mut io);
    let part = parts.get(part_idx).unwrap_or(&parts[0]);
    match part.fs {
        FsKind::ExFat => fat::analyze_exfat(&mut io, part.start_offset),
        _ => fat::analyze_fat(&mut io, part.start_offset),
    }
}

fn run_dump_ext(path: &str, part_idx: usize) {
    let mut file = File::open(path).expect("Failed to open file");
    let mut io = StdRimIO::new(&mut file);
    let parts = scan_partitions(&mut io);
    let part = parts.get(part_idx).unwrap_or(&parts[0]);
    ext::analyze_ext(&mut io, part.start_offset);
}

fn print_report(report: &VerifyReport) {
    println!("\n=== Filesystem Integrity Report ===");
    let errors = report.findings.iter().filter(|f| f.sev == Severity::Error).count();
    let warns = report.findings.iter().filter(|f| f.sev == Severity::Warn).count();
    let infos = report.findings.iter().filter(|f| f.sev == Severity::Info).count();

    for f in &report.findings {
        let tag = match f.sev {
            Severity::Error => "[ERROR]",
            Severity::Warn => "[WARN ]",
            Severity::Info => "[INFO ]",
        };
        println!("{} [{}] {}", tag, f.code, f.msg);
    }

    println!("\nSummary: {} errors, {} warnings, {} checks performed", errors, warns, infos);
    if errors == 0 {
        println!("[+] Filesystem is structurally CLEAN.");
    } else {
        println!("[-] Filesystem has INTEGRITY VIOLATIONS!");
    }
}

fn find_partition_offset(io: &mut StdRimIO<File>) -> u64 {
    let parts = scan_partitions(io);
    parts.first().map(|p| p.start_offset).unwrap_or(0)
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
        ("EXT Superblock", &[0x53, 0xEF]),
        ("ISO 9660", b"CD001"),
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
    let off1 = find_partition_offset(&mut io1);
    let off2 = find_partition_offset(&mut io2);
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
