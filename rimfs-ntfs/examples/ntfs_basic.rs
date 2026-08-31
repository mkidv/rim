// SPDX-License-Identifier: MIT

use rimfs_ntfs::prelude::*;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const SIZE_BYTES: u64 = 32 * 1024 * 1024;

    let mut image = vec![0u8; SIZE_BYTES as usize];
    let mut io = MemRimIO::new(&mut image);
    let meta = NtfsMeta::new(SIZE_BYTES, Some("RIMNTFS"))?;
    let align = meta.bytes_per_cluster as u64;
    let total = Instant::now();

    let mut format_io = IOCounter::with_align(&mut io, align);
    let t = Instant::now();
    NtfsFormatter::new(&mut format_io, &meta).format(false)?;
    let format = (t.elapsed(), format_io.snapshot());

    let mut tree = FsNode::new_container(vec![FsNode::new_file(
        "hello.txt",
        b"hello from NTFS\n".to_vec(),
    )]);
    let mut inject_io = IOCounter::with_align(format_io.into_inner(), align);
    let t = Instant::now();
    let mut injector = NtfsInjector::new(&mut inject_io, &meta)?;
    injector.inject_tree(&mut tree)?;
    injector.flush()?;
    let inject = (t.elapsed(), inject_io.snapshot());

    let mut check_io = IOCounter::with_align(inject_io.into_inner(), align);
    let t = Instant::now();
    let report = NtfsChecker::new(&mut check_io, &meta).check_all()?;
    let check = (t.elapsed(), check_io.snapshot());
    assert!(!report.has_error());

    let mut resolve_io = IOCounter::with_align(check_io.into_inner(), align);
    let t = Instant::now();
    let mut resolver = NtfsResolver::new(&mut resolve_io, &meta);
    assert_eq!(resolver.read_file("hello.txt")?, b"hello from NTFS\n");
    let resolve = (t.elapsed(), resolve_io.snapshot());

    println!("NTFS example completed in {:?}", total.elapsed());
    println!("  format : {:?} | {}", format.0, format.1);
    println!("  inject : {:?} | {}", inject.0, inject.1);
    println!("  check  : {:?} | {}", check.0, check.1);
    println!("  resolve: {:?} | {}", resolve.0, resolve.1);
    Ok(())
}
