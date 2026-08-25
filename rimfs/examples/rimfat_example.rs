// SPDX-License-Identifier: MIT

use rimfs::{
    core::{checker::ReportDisplayOpts, resolver::FsTreeDisplayOpts},
    fat::*,
};
use std::{path::PathBuf, time::Instant};

fn main() {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data/*");
    let test_data_path = test_data_dir.to_str().unwrap();
    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    println!("=== RimFAT (FAT32 Extended) Example - {SIZE_MB} MB image ===");
    println!("Features: LFN Protection, No-FAT-Chain, Global Integrity, Transactions\n");

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem = MemRimIO::new(&mut buf);

    // 0. CREATE METADATA
    let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("RIMFAT")).unwrap();
    let align = meta.bytes_per_cluster as u64;

    // 1. FORMAT
    let t0 = Instant::now();
    let mut io_for_format = IOCounter::with_align(&mut mem, align);
    let mut formatter = FatFormatter::new(&mut io_for_format, &meta);
    formatter.format(false).expect("format failed");
    let dt_format = t0.elapsed();
    let stats_format = io_for_format.snapshot();

    // 2. PARSE HOST DATA (Benchmark only)
    let t1 = Instant::now();
    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");
    let dt_parse_std = t1.elapsed();

    // 3. INJECT WITH RIM-INTEGRITY
    let mut io_for_inject = IOCounter::with_align(io_for_format.into_inner(), align);
    let t2 = Instant::now();

    let mut injector = FatInjector::new(&mut io_for_inject, &meta).expect("injector failed");
    injector.inject_tree(&tree).expect("inject failed");
    injector.flush().expect("flush failed");

    let dt_inject = t2.elapsed();
    let stats_inject = io_for_inject.snapshot();

    // 4. CHECK INTEGRITY (BEFORE CORRUPTION)
    let mut io_for_check = IOCounter::with_align(io_for_inject.into_inner(), align);
    let t3 = Instant::now();
    let mut checker = FatChecker::new(&mut io_for_check, &meta);
    let report = checker.check_all().expect("check failed");
    let dt_check = t3.elapsed();
    let stats_check = io_for_check.snapshot();

    // 5. PARSE BACK (FAST-PATH)
    let mut io_for_parse_back = IOCounter::with_align(io_for_check.into_inner(), align);
    let t4 = Instant::now();
    let node = {
        let mut resolver = FatResolver::new(&mut io_for_parse_back, &meta);
        resolver.parse_tree("/*").expect("parse_tree failed")
    };
    let dt_parse_fat = t4.elapsed();
    let stats_parse_fat = io_for_parse_back.snapshot();

    let total = t0.elapsed();

    // DISPLAY RESULTS
    println!("\nDurations:");
    println!("  Total        : {total:?}");
    println!("  Format       : {dt_format:?}");
    println!("  Parse (host) : {dt_parse_std:?}");
    println!("  Inject       : {dt_inject:?}");
    println!("  Check        : {dt_check:?}");
    println!("  Parse (img)  : {dt_parse_fat:?}");

    println!("\nIO stats:");
    println!("  Format       : {stats_format}");
    println!("  Inject       : {stats_inject}");
    println!("  Check        : {stats_check}");
    println!("  Parse (img)  : {stats_parse_fat}");

    println!("\nCheck report (PRE-CORRUPTION):");
    println!(
        "{}",
        report.display_with(ReportDisplayOpts {
            prefix: "  ",
            ..ReportDisplayOpts::default()
        })
    );

    println!(
        "\nOn disk structure:\n{}",
        node.display_with(FsTreeDisplayOpts {
            max_lines: 0,
            ..FsTreeDisplayOpts::default()
        })
    );

    // 6. DEMONSTRATE INTEGRITY (CORRUPTION DETECT)
    println!("\n--- DEMONSTRATING RIM-FAT INTEGRITY ---");
    let io_raw = io_for_parse_back.into_inner();

    println!("\n-> Scenario 1: Metadata Entry Corruption");
    let root_off = meta.unit_offset(meta.root_unit());
    let mut entry_buffer = [0u8; 32];
    let entry_off = root_off + 32;
    io_raw.read_at(entry_off, &mut entry_buffer).unwrap();

    println!("   Original entry name[0]: 0x{:02X}", entry_buffer[0]);
    entry_buffer[0] ^= 0xFF; // Corrupt
    io_raw.write_at(entry_off, &entry_buffer).unwrap();

    let mut io_for_demo1 = IOCounter::new(io_raw);
    {
        let mut resolver = FatResolver::new(&mut io_for_demo1, &meta);
        match resolver.read_dir("/") {
            Err(e) => println!("   SUCCESS: Detected metadata corruption (CRC mismatch): {e:?}"),
            Ok(_) => println!("   FAILURE: Corruption went undetected!"),
        }
    }

    println!("\n-> Scenario 2: Global FAT Integrity Corruption");
    let io_raw_2 = io_for_demo1.into_inner();
    let fat_off = meta.fat_offset_bytes;
    let mut fat_byte = [0u8; 1];
    io_raw_2.read_at(fat_off + 10, &mut fat_byte).unwrap();
    fat_byte[0] ^= 0xAA;
    io_raw_2.write_at(fat_off + 10, &fat_byte).unwrap();

    match FatMeta::from_io(io_raw_2) {
        Err(e) => println!("   SUCCESS: Detected FAT table corruption: {e:?}"),
        Ok(m) => {
            println!("   FAILURE: FAT corruption went undetected!");
            if m.is_dirty {
                println!("   [!] Note: Volume is marked as DIRTY.");
            }
        }
    }

    println!("\n=== RimFAT Example Complete ===");
}
