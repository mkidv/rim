// SPDX-License-Identifier: MIT

use rimfs::exfat::*;
use rimio::prelude::*; // <-- pour IOCounter, IoStats
use std::{path::PathBuf, time::Instant};

fn print_io_stats(label: &str, s: &IoStats) {
    println!(
        "[{label}] R={} ({} B) W={} ({} B) F={} | aligned R/W = {}/{} | unaligned R/W = {}/{} | maxRead={}B maxWrite={}B",
        s.reads, s.read_bytes, s.writes, s.write_bytes, s.flushes,
        s.aligned_reads, s.aligned_writes, s.unaligned_reads, s.unaligned_writes,
        s.max_read, s.max_write
    );
}

fn main() {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data/*");
    let test_data_path = test_data_dir.to_str().unwrap();
    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    println!("=== ExFAT Test (instrumented) ===");
    println!("Allocating {SIZE_MB} MB image...");

    // backend RAM
    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem = MemBlockIO::new(&mut buf);
    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCHFS"));

    // On choisit une granularité d’alignement pertinente pour la mesure.
    // Ici: clusters exFAT (ou 4096 pour simuler 4K disque).
    let align = meta.bytes_per_cluster as u64; // ou 4096
    // Compteur dédié au FORMAT
    let mut io_for_format = IOCounter::with_align(&mut mem, align);

    // 1) FORMAT
    let t0 = Instant::now();
    let mut formatter = ExFatFormatter::new(&mut io_for_format, &meta);
    formatter.format(false).expect("format failed");
    let dt_fmt = t0.elapsed();
    let fmt_stats = io_for_format.snapshot();
    print_io_stats("format", &fmt_stats);

    // 2) PARSE FS SOURCE (host)
    let t1 = Instant::now();
    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");
    let dt_parse_std = t1.elapsed();

    // 3) INJECT — nouveau compteur pour isoler la phase
    let mut io_for_inject = IOCounter::with_align(io_for_format.into_inner(), align);
    let mut allocator = ExFatAllocator::new(&meta);
    let t2 = Instant::now();
    let mut injector = ExFatInjector::new(&mut io_for_inject, &mut allocator, &meta);
    injector.inject_tree(&tree).expect("inject failed");
    let dt_inject = t2.elapsed();
    let inj_stats = io_for_inject.snapshot();
    print_io_stats("inject", &inj_stats);

    // 4) CHECK (optionnel)
    let mut io_for_check = IOCounter::with_align(io_for_inject.into_inner(), align);
    let t3 = Instant::now();
    let mut checker = ExFatChecker::new(&mut io_for_check, &meta);
    let report = checker.check_all().expect("check failed");
    let dt_check = t3.elapsed();
    let chk_stats = io_for_check.snapshot();
    print_io_stats("check", &chk_stats);

    // 5) PARSE BACK (depuis l’image)
    let mut io_for_parse_back = IOCounter::with_align(io_for_check.into_inner(), align);
    let t4 = Instant::now();
    let mut resolver = ExFatResolver::new(&mut io_for_parse_back, &meta);
    let node = resolver.parse_tree("/*").expect("parse_tree failed");
    let dt_parse_fat = t4.elapsed();
    let pback_stats = io_for_parse_back.snapshot();
    print_io_stats("parse_back", &pback_stats);

    let total = t0.elapsed();

    println!("\nSummary:");
    println!("Total        : {total:?}");
    println!("Format       : {dt_fmt:?}");
    println!("Parse (host) : {dt_parse_std:?}");
    println!("Inject       : {dt_inject:?}");
    println!("Check        : {dt_check:?}");
    println!("Parse (img)  : {dt_parse_fat:?}");
    println!("Check report :\n{report}");
    println!("On disk :\n{node}");
}
