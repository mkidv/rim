use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs_core::resolver::{attr::FileAttributes, node::FsNode};
use rimfs_fat::prelude::*;

const SIZE_MB: u64 = 64;
const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
const WRITE_SIZE: usize = 10 * 1024 * 1024;
const NUM_FILES: usize = 100;
const FILE_SIZE: usize = 1024;

fn bench_rimfat_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("rimfat_format");
    group.throughput(Throughput::Bytes(SIZE_BYTES));

    group.bench_function("format_64mb_mem", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_disk", |b| {
        b.iter(|| {
            let mut file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_mmap", |b| {
        b.iter(|| {
            let file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = MmapRimIO::new(file).unwrap();
            let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_rimfat_large_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("rimfat_write_large");
    let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xAAu8; WRITE_SIZE];
    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));

    group.bench_function("write_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = FatInjector::new(&mut io, &meta).unwrap();
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
                FatFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                let mut node = FsNode::new_file("large.bin", content_copy);
                injector.inject_tree(&mut node).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_rimfat_large_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("rimfat_read_large");
    let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = FatInjector::new(&mut io, &meta).unwrap();
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
                let mut resolver = FatResolver::new(&mut io, &meta);
                let data = resolver.read_file("/large.bin").unwrap();
                assert_eq!(data.len(), WRITE_SIZE);
            },
        );
    });

    group.finish();
}

fn bench_rimfat_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("rimfat_small_files");
    let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
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
                let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                injector.inject_tree(&mut local_tree).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_rimfat_check(c: &mut Criterion) {
    let mut group = c.benchmark_group("rimfat_check");
    let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = FatInjector::new(&mut io, &meta).unwrap();
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
                let mut checker = FatChecker::new(&mut io, &meta);
                let report = checker.check_all().unwrap();
                assert!(!report.has_error());
            },
        );
    });

    group.finish();
}

fn bench_rimfat_resolve_tree(c: &mut Criterion) {
    let mut group = c.benchmark_group("rimfat_resolve_tree");
    let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH_RIMFAT")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = FatInjector::new(&mut io, &meta).unwrap();
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
                let mut resolver = FatResolver::new(&mut io, &meta);
                let node = resolver.resolve_tree("/*").unwrap();
                assert_eq!(node.counts().files, NUM_FILES);
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_rimfat_format,
    bench_rimfat_large_write,
    bench_rimfat_large_read,
    bench_rimfat_small_files,
    bench_rimfat_check,
    bench_rimfat_resolve_tree,
);
criterion_main!(benches);
