use criterion::{Criterion, criterion_group, criterion_main};
use std::path::PathBuf;

use rimfs::fat32::*;

criterion_group!(benches, fat32_component_bench);
criterion_main!(benches);

pub fn fat32_bench(c: &mut Criterion) {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();
    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");
    let meta = Fat32Meta::new(SIZE_BYTES, Some("BENCHFS"));

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem_io = MemBlockIO::new(&mut buf);

    c.bench_function("fat32_format_inject_mem", |b| {
        b.iter(|| {
            let mut formatter = Fat32Formatter::new(&mut mem_io, &meta);
            formatter.format(false).expect("format failed");
            let mut allocator = Fat32Allocator::new(&meta);
            let mut injector = Fat32Injector::new(&mut mem_io, &mut allocator, &meta);
            injector.inject_tree(&tree).expect("inject failed");
        });
    });

    let mut file = tempfile::tempfile().expect("tempfile failed");
    file.set_len(SIZE_BYTES).expect("set_len failed");
    let mut temp_io = StdBlockIO::new(&mut file);

    c.bench_function("fat32_format_inject_file", |b| {
        b.iter(|| {
            let mut formatter = Fat32Formatter::new(&mut temp_io, &meta);
            formatter.format(false).expect("format failed");
            let mut allocator = Fat32Allocator::new(&meta);
            let mut injector = Fat32Injector::new(&mut temp_io, &mut allocator, &meta);
            injector.inject_tree(&tree).expect("inject failed");
        });
    });
}

pub fn fat32_component_bench(c: &mut Criterion) {
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();

    const SIZE_MB: u64 = 32;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    let mut buf = vec![0u8; SIZE_BYTES as usize];
    let mut mem_io = MemBlockIO::new(&mut buf);
    let meta = Fat32Meta::new(SIZE_BYTES, Some("BENCHFS"));

    c.bench_function("fat32_format", |b| {
        b.iter(|| {
            let mut formatter = Fat32Formatter::new(&mut mem_io, &meta);
            formatter.format(false).expect("format failed");
        });
    });

    let mut formatter = Fat32Formatter::new(&mut mem_io, &meta);
    formatter.format(false).expect("format failed");

    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).unwrap();

    c.bench_function("fat32_inject", |b| {
        b.iter(|| {
            let mut allocator = Fat32Allocator::new(&meta);
            let mut injector = Fat32Injector::new(&mut mem_io, &mut allocator, &meta);
            injector.inject_tree(&tree).unwrap();
        });
    });

    let mut allocator: Fat32Allocator<'_> = Fat32Allocator::new(&meta);
    let mut injector = Fat32Injector::new(&mut mem_io, &mut allocator, &meta);
    injector.inject_tree(&tree).unwrap();

    c.bench_function("fat32_parse", |b| {
        b.iter(|| {
            let mut parser_back = Fat32Resolver::new(&mut mem_io, &meta);
            let _node = parser_back.parse_tree("/*").unwrap();
        });
    });

    c.bench_function("fat32_check", |b| {
        b.iter(|| {
            let mut checker = Fat32Checker::new(&mut mem_io, &meta);
            checker.check_all().unwrap();
        });
    });
}

pub fn fat32_scaling_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_scaling");
    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();

    let mut parser = StdResolver::new();
    let tree = parser.parse_tree(test_data_path).expect("parse failed");

    for &size_mb in &[16u64, 32, 64, 128, 256] {
        let size_bytes = size_mb * 1024 * 1024;
        group.bench_with_input(
            format!("format_inject_{size_mb}MB_std"),
            &size_bytes,
            |b, &sz| {
                b.iter(|| {
                    let mut buf = vec![0u8; sz as usize];
                    let mut mem_io = MemBlockIO::new(&mut buf);
                    let meta = Fat32Meta::new(sz, Some("SCALEFS"));
                    let mut formatter = Fat32Formatter::new(&mut mem_io, &meta);
                    formatter.format(false).expect("format failed");
                    let mut allocator = Fat32Allocator::new(&meta);
                    let mut injector = Fat32Injector::new(&mut mem_io, &mut allocator, &meta);
                    injector.inject_tree(&tree).expect("inject failed");
                })
            },
        );
    }
    group.finish();
}

pub fn fat32_component_scaling_bench(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_component_scaling");

    let test_data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data");
    let test_data_path = test_data_dir.to_str().unwrap();

    for &size_mb in &[16u64, 32, 64, 128, 256] {
        let size_bytes = size_mb * 1024 * 1024;

        // FORMAT
        group.bench_with_input(format!("format_{size_mb}MB"), &size_bytes, |b, &sz| {
            b.iter(|| {
                let mut buf = vec![0u8; sz as usize];
                let mut mem_io = MemBlockIO::new(&mut buf);
                let meta = Fat32Meta::new(sz, Some("BENCHFS"));
                let mut formatter = Fat32Formatter::new(&mut mem_io, &meta);
                formatter.format(false).expect("format failed");
            });
        });

        // PARSE STD
        let mut buf = vec![0u8; size_bytes as usize];
        let mut mem_io = MemBlockIO::new(&mut buf);
        let meta = Fat32Meta::new(size_bytes, Some("BENCHFS"));

        let mut formatter = Fat32Formatter::new(&mut mem_io, &meta);
        formatter.format(false).expect("format failed");

        let mut parser = StdResolver::new();
        let tree = parser.parse_tree(test_data_path).unwrap();

        group.bench_with_input(format!("inject_{size_mb}MB"), &size_bytes, |b, &sz| {
            b.iter(|| {
                let mut allocator = Fat32Allocator::new(&meta);
                let mut injector = Fat32Injector::new(&mut mem_io, &mut allocator, &meta);
                injector.inject_tree(&tree).unwrap();
            });
        });

        // parse_fat → ne pas toucher MemBlockIO dans iter
        group.bench_with_input(format!("parse_fat_{size_mb}MB"), &size_bytes, |b, _| {
            b.iter(|| {
                let mut parser_back = Fat32Resolver::new(&mut mem_io, &meta);
                let _node = parser_back.parse_tree("/*").unwrap();
            });
        });

        let mut allocator = Fat32Allocator::new(&meta);
        let mut injector = Fat32Injector::new(&mut mem_io, &mut allocator, &meta);
        injector.inject_tree(&tree).unwrap();

        // check → idem
        group.bench_with_input(format!("check_{size_mb}MB"), &size_bytes, |b, _| {
            b.iter(|| {
                let mut checker = Fat32Checker::new(&mut mem_io, &meta);
                checker.check_all().unwrap();
            });
        });
    }

    group.finish();
}
