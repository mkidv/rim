// SPDX-License-Identifier: MIT

use rimfs_tar::prelude::*;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const SIZE_BYTES: u64 = 1024 * 1024;

    let mut archive = vec![0u8; SIZE_BYTES as usize];
    let mut io = MemRimIO::new(&mut archive);
    let meta = TarMeta::new(SIZE_BYTES, Some("RIMTAR"))?;
    let align = TAR_BLOCK_SIZE as u64;
    let total = Instant::now();

    let mut format_io = IOCounter::with_align(&mut io, align);
    let t = Instant::now();
    TarFormatter::new(&mut format_io, &meta).format(false)?;
    let format = (t.elapsed(), format_io.snapshot());

    let mut tree = FsNode::new_container(vec![
        FsNode::new_file("hello.txt", b"hello from TAR\n".to_vec()),
        FsNode::new_symlink("hello.link", "hello.txt"),
    ]);
    let mut inject_io = IOCounter::with_align(format_io.into_inner(), align);
    let t = Instant::now();
    let mut injector = TarInjector::new(&mut inject_io, &meta)?;
    injector.inject_tree(&mut tree)?;
    injector.flush()?;
    let inject = (t.elapsed(), inject_io.snapshot());

    let mut check_io = IOCounter::with_align(inject_io.into_inner(), align);
    let t = Instant::now();
    let report = TarChecker::new(&mut check_io, &meta).check_all()?;
    let check = (t.elapsed(), check_io.snapshot());
    assert!(!report.has_error());

    let mut resolve_io = IOCounter::with_align(check_io.into_inner(), align);
    let t = Instant::now();
    let mut resolver = TarResolver::new(&mut resolve_io, &meta);
    assert_eq!(resolver.read_file("hello.txt")?, b"hello from TAR\n");
    assert_eq!(resolver.read_link("hello.link")?, "hello.txt");
    let resolve = (t.elapsed(), resolve_io.snapshot());

    println!("TAR example completed in {:?}", total.elapsed());
    println!("  format : {:?} | {}", format.0, format.1);
    println!("  inject : {:?} | {}", inject.0, inject.1);
    println!("  check  : {:?} | {}", check.0, check.1);
    println!("  resolve: {:?} | {}", resolve.0, resolve.1);
    Ok(())
}
