// SPDX-License-Identifier: MIT

use rimfs::exfat::*;
use std::{path::PathBuf, time::Instant};

fn main() {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data/*");
    let test_data_path = test_data_dir.to_str().unwrap();
    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    println!("=== ExFat Bench ===");
    println!("Allocating {SIZE_MB} MB image...");

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut io = MemBlockIO::new(&mut buf);
    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCHFS"));

    // 1. Format
    let t0 = Instant::now();
    let mut formatter = ExFatFormatter::new(&mut io, &meta);
    formatter.format(false).expect("format failed");
    let dt_fmt = t0.elapsed();

    // 2. Parse content from current folder
    let t1 = Instant::now();
    let mut parser = StdFsParser::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");
    let dt_parse_std = t1.elapsed();

    // 3. Inject parsed content
    let t2 = Instant::now();
    let mut allocator = ExFatAllocator::new(&meta);
    let mut injector = ExFatInjector::new(&mut io, &mut allocator, &meta);
    injector.inject_tree(&tree).expect("inject failed");

    let dt_inject = t2.elapsed();
    println!("Inject done");

    let t3 = Instant::now();
    let mut checker = ExFatChecker::new(&mut io, &meta);
    checker.check_all().expect("check failed");
    let dt_check = t3.elapsed();
    println!("Check done");

    // 5. Parse back to verify / benchmark parsing speed
    let mut parser_back = ExFatParser::new(&mut io, &meta);
    let t4 = Instant::now();
    let node = parser_back.parse_tree("/*").expect("parse_tree failed");
    let dt_parse_fat = t4.elapsed();

    let dt_process = t0.elapsed();

    // 6. Summary
    println!("Summary:");
    println!("Total time   : {dt_process:?}");
    println!("Format time   : {dt_fmt:?}");
    println!("Parsing tree time  : {dt_parse_std:?}");
    println!("Injection time: {dt_inject:?}");
    println!("Check time: {dt_check:?}");
    println!("Parsing fat time  : {dt_parse_fat:?}");
    println!("On disk :");
    println!("{node}");
}
