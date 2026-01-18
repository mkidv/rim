// SPDX-License-Identifier: MIT

use rimfs::{
    core::{checker::ReportDisplayOpts, resolver::FsTreeDisplayOpts},
    fat32::*,
};
use std::{path::PathBuf, time::Instant};

fn main() {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data/*");
    let test_data_path = test_data_dir.to_str().unwrap();
    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    println!("=== FAT32 Test - {SIZE_MB} MB image ===");

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem = MemRimIO::new(&mut buf);
    let meta = Fat32Meta::new(SIZE_BYTES, Some("BENCHFS")).unwrap();

    // Alignement pertinent : cluster FAT32, sinon 4096.
    let align = meta.bytes_per_cluster as u64; // ou 4096
    let mut io_for_format = IOCounter::with_align(&mut mem, align);

    // FORMAT
    let t0 = Instant::now();
    let mut formatter = Fat32Formatter::new(&mut io_for_format, &meta);
    formatter.format(false).expect("format failed");
    let dt_format = t0.elapsed();
    let stats_format = io_for_format.snapshot();

    // PARSE (host)
    let t1 = Instant::now();
    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");
    let dt_parse_std = t1.elapsed();

    // INJECT
    let mut io_for_inject = IOCounter::with_align(io_for_format.into_inner(), align);
    let mut allocator = Fat32Allocator::new(&meta);
    let t2 = Instant::now();
    let mut injector = Fat32Injector::new(&mut io_for_inject, &mut allocator, &meta);
    injector.inject_tree(&tree).expect("inject failed");
    let dt_inject = t2.elapsed();
    let stats_inject = io_for_inject.snapshot();

    // CHECK
    let mut io_for_check = IOCounter::with_align(io_for_inject.into_inner(), align);
    let t3 = Instant::now();
    let mut checker = Fat32Checker::new(&mut io_for_check, &meta);
    let report = checker.check_all().expect("check failed");
    let dt_check = t3.elapsed();
    let stats_check = io_for_check.snapshot();

    // PARSE BACK
    let mut io_for_parse_back = IOCounter::with_align(io_for_check.into_inner(), align);
    let t4 = Instant::now();
    let mut resolver = Fat32Resolver::new(&mut io_for_parse_back, &meta);
    let node = resolver.parse_tree("/*").expect("parse_tree failed");
    let dt_parse_fat = t4.elapsed();
    let stats_parse_fat = io_for_parse_back.snapshot();

    let total = t0.elapsed();

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

    println!("\nCheck report:");
    println!(
        "{}",
        report.display_with(ReportDisplayOpts {
            prefix: "  ",
            ..ReportDisplayOpts::default()
        })
    );

    println!(
        "\nOn disk :\n{}",
        node.display_with(FsTreeDisplayOpts {
            max_lines: 0,
            ..FsTreeDisplayOpts::default()
        })
    );
}
