// SPDX-License-Identifier: MIT
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs_core::resolver::{attr::FileAttributes, node::FsNode};
use rimfs_zip::prelude::*;

const SIZE_MB: u64 = 64;
const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
const WRITE_SIZE: usize = 10 * 1024 * 1024;
const NUM_FILES: usize = 100;
const FILE_SIZE: usize = 1024;

fn bench_zip_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("zip_format");
    group.throughput(Throughput::Bytes(SIZE_BYTES));

    group.bench_function("format_64mb_mem", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
            ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_disk", |b| {
        b.iter(|| {
            let mut file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
            ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_mmap", |b| {
        b.iter(|| {
            let file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = unsafe { MmapRimIO::new(file) }.unwrap();
            let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
            ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_zip_large_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("zip_write_large");
    let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ZipFormatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xAAu8; WRITE_SIZE];
    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));

    group.bench_function("write_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
                let mut node = FsNode::new_file("large.bin", content_copy);
                injector.inject_tree(&mut node).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.bench_function("write_10mb_contiguous_disk", |b| {
        b.iter_with_setup(
            || {
                let mut file = tempfile::tempfile().unwrap();
                file.set_len(SIZE_BYTES).unwrap();
                let mut io = StdRimIO::new(&mut file);
                ZipFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
                let mut node = FsNode::new_file("large.bin", content_copy);
                injector.inject_tree(&mut node).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_zip_large_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("zip_read_large");
    let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
        let content = vec![0xAAu8; WRITE_SIZE];
        let mut node = FsNode::new_file("large.bin", content);
        injector.inject_tree(&mut node).unwrap();
        injector.flush().unwrap();
    }

    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));

    group.bench_function("read_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || disk_buf.clone(),
            |mut local_buf| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut resolver = ZipResolver::new(&mut io, &meta);
                let data = resolver.read_file("/large.bin").unwrap();
                assert_eq!(data.len(), WRITE_SIZE);
            },
        );
    });

    group.finish();
}

fn bench_zip_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("zip_small_files");
    let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ZipFormatter::new(&mut io, &meta).format(false).unwrap();
    }

    group.throughput(Throughput::Elements(NUM_FILES as u64));

    group.bench_function("create_100_small_files_mem", |b| {
        b.iter_with_setup(
            || {
                let files: Vec<FsNode> = (0..NUM_FILES)
                    .map(|i| FsNode::new_file(format!("file_{i}.txt"), vec![0xBB; FILE_SIZE]))
                    .collect();
                let tree = FsNode::Container {
                    attr: FileAttributes::new_dir(),
                    children: files,
                };
                (disk_buf.clone(), tree)
            },
            |(mut local_buf, mut local_tree)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
                injector.inject_tree(&mut local_tree).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_zip_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("zip_check");
    let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
        let files: Vec<FsNode> = (0..NUM_FILES)
            .map(|i| FsNode::new_file(format!("file_{i}.txt"), vec![0xBB; FILE_SIZE]))
            .collect();
        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: files,
        };
        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();
    }

    group.throughput(Throughput::Elements(NUM_FILES as u64));

    group.bench_function("check_100_files_mem", |b| {
        b.iter_with_setup(
            || disk_buf.clone(),
            |mut local_buf| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut checker = ZipChecker::new(&mut io, &meta);
                let report = checker.check_all().unwrap();
                assert!(!report.has_error());
            },
        );
    });

    group.finish();
}

fn bench_zip_resolve_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("zip_resolve_tree");
    let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH_ZIP")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
        let files: Vec<FsNode> = (0..NUM_FILES)
            .map(|i| FsNode::new_file(format!("file_{i}.txt"), vec![0xBB; FILE_SIZE]))
            .collect();
        let mut tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: files,
        };
        injector.inject_tree(&mut tree).unwrap();
        injector.flush().unwrap();
    }

    group.throughput(Throughput::Elements(NUM_FILES as u64));

    group.bench_function("resolve_tree_100_files_mem", |b| {
        b.iter_with_setup(
            || disk_buf.clone(),
            |mut local_buf| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut resolver = ZipResolver::new(&mut io, &meta);
                let node = resolver.resolve_tree("/*").unwrap();
                assert_eq!(node.counts().files, NUM_FILES);
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_zip_format,
    bench_zip_large_write,
    bench_zip_large_read,
    bench_zip_small_files,
    bench_zip_check,
    bench_zip_resolve_tree,
);
criterion_main!(benches);
