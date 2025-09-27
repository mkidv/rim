use criterion::{Criterion, criterion_group, criterion_main};
use std::path::PathBuf;

use rimfs::exfat::*;

criterion_group!(benches, exfat_bench);
criterion_main!(benches);

pub fn exfat_bench(c: &mut Criterion) {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();
    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");
    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCHFS"));

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem_io = MemBlockIO::new(&mut buf);

    c.bench_function("exfat_format_inject_mem", |b| {
        b.iter(|| {
            let mut formatter = ExFatFormatter::new(&mut mem_io, &meta);
            formatter.format(false).expect("format failed");
            let mut allocator = ExFatAllocator::new(&meta);
            let mut injector = ExFatInjector::new(&mut mem_io, &mut allocator, &meta);
            injector.inject_tree(&tree).expect("inject failed");
        });
    });

    let mut file = tempfile::tempfile().expect("tempfile failed");
    file.set_len(SIZE_BYTES).expect("set_len failed");
    let mut temp_io = StdBlockIO::new(&mut file);

    c.bench_function("exfat_format_inject_file", |b| {
        b.iter(|| {
            let mut formatter = ExFatFormatter::new(&mut temp_io, &meta);
            formatter.format(false).expect("format failed");
            let mut allocator = ExFatAllocator::new(&meta);
            let mut injector = ExFatInjector::new(&mut temp_io, &mut allocator, &meta);
            injector.inject_tree(&tree).expect("inject failed");
        });
    });
}

pub fn exfat_component_bench(c: &mut Criterion) {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem_io = MemBlockIO::new(&mut buf);
    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCHFS"));

    c.bench_function("exfat_format", |b| {
        b.iter(|| {
            let mut formatter = ExFatFormatter::new(&mut mem_io, &meta);
            formatter.format(false).expect("format failed");
        });
    });

    let mut formatter = ExFatFormatter::new(&mut mem_io, &meta);
    formatter.format(false).expect("format failed");

    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).unwrap();

    c.bench_function("exfat_inject", |b| {
        b.iter(|| {
            let mut allocator = ExFatAllocator::new(&meta);
            let mut injector = ExFatInjector::new(&mut mem_io, &mut allocator, &meta);
            injector.inject_tree(&tree).unwrap();
        });
    });

    let mut allocator: ExFatAllocator<'_> = ExFatAllocator::new(&meta);
    let mut injector = ExFatInjector::new(&mut mem_io, &mut allocator, &meta);
    injector.inject_tree(&tree).unwrap();

    c.bench_function("exfat_parse", |b| {
        b.iter(|| {
            let mut parser_back = ExFatResolver::new(&mut mem_io, &meta);
            let _node = parser_back.parse_tree("/*").unwrap();
        });
    });

    c.bench_function("exfat_check", |b| {
        b.iter(|| {
            let mut checker = ExFatChecker::new(&mut mem_io, &meta);
            checker.check_all().unwrap();
        });
    });
}

pub fn exfat_scaling_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("exfat_scaling");
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();

    for &size_mb in &[16u64, 32, 64, 128, 256] {
        let size_bytes = size_mb * 1024 * 1024;
        group.bench_with_input(
            format!("format_inject_parse_{size_mb}MB_std"),
            &size_bytes,
            |b, &sz| {
                b.iter(|| {
                    let mut buf = vec![0u8; sz as usize];
                    let mut mem_io = MemBlockIO::new(&mut buf);
                    let meta = ExFatMeta::new(sz, Some("SCALEFS"));
                    let mut formatter = ExFatFormatter::new(&mut mem_io, &meta);
                    formatter.format(false).expect("format failed");
                    let mut parser = StdResolver::new();
                    let tree = parser.parse_tree(test_data_path).expect("parse failed");
                    let mut allocator = ExFatAllocator::new(&meta);
                    let mut injector = ExFatInjector::new(&mut mem_io, &mut allocator, &meta);
                    injector.inject_tree(&tree).expect("inject failed");
                })
            },
        );
    }
    group.finish();
}

pub fn exfat_component_scaling_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("exfat_component_scaling");

    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();

    for &size_mb in &[16u64, 32, 64, 128, 256] {
        let size_bytes = size_mb * 1024 * 1024;

        // FORMAT
        group.bench_with_input(format!("format_{size_mb}MB"), &size_bytes, |b, &sz| {
            b.iter(|| {
                let mut buf = vec![0u8; sz as usize];
                let mut mem_io = MemBlockIO::new(&mut buf);
                let meta = ExFatMeta::new(sz, Some("BENCHFS"));
                let mut formatter = ExFatFormatter::new(&mut mem_io, &meta);
                formatter.format(false).expect("format failed");
            });
        });

        // PARSE STD
        let mut buf = vec![0u8; size_bytes as usize];
        let mut mem_io = MemBlockIO::new(&mut buf);
        let meta = ExFatMeta::new(size_bytes, Some("BENCHFS"));

        let mut formatter = ExFatFormatter::new(&mut mem_io, &meta);
        formatter.format(false).expect("format failed");

        let mut parser = StdResolver::new();
        let tree = parser.parse_tree(test_data_path).unwrap();

        group.bench_with_input(format!("inject_{size_mb}MB"), &size_bytes, |b, &sz| {
            b.iter(|| {
                let mut allocator = ExFatAllocator::new(&meta);
                let mut injector = ExFatInjector::new(&mut mem_io, &mut allocator, &meta);
                injector.inject_tree(&tree).unwrap();
            });
        });

        // parse_fat → ne pas toucher MemBlockIO dans iter
        group.bench_with_input(format!("parse_fat_{size_mb}MB"), &size_bytes, |b, _| {
            b.iter(|| {
                let mut parser_back = ExFatResolver::new(&mut mem_io, &meta);
                let _node = parser_back.parse_tree("/*").unwrap();
            });
        });

        let mut allocator = ExFatAllocator::new(&meta);
        let mut injector = ExFatInjector::new(&mut mem_io, &mut allocator, &meta);
        injector.inject_tree(&tree).unwrap();

        // check → idem
        group.bench_with_input(format!("check_{size_mb}MB"), &size_bytes, |b, _| {
            b.iter(|| {
                let mut checker = ExFatChecker::new(&mut mem_io, &meta);
                checker.check_all().unwrap();
            });
        });
    }

    group.finish();
}
