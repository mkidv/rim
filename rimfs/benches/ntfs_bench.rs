use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs::ntfs::*;

fn bench_ntfs_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("ntfs_format");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    group.throughput(Throughput::Bytes(SIZE_BYTES));
    group.bench_function("format_64mb_mem", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            NtfsFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_disk", |b| {
        b.iter(|| {
            let mut file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            NtfsFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_mmap", |b| {
        b.iter(|| {
            let file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = MmapRimIO::new(file).unwrap();
            let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            NtfsFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_ntfs_large_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("ntfs_write_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();
    }

    let content = vec![0xAAu8; WRITE_SIZE];

    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));
    group.bench_function("write_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();

                let node = FsNode::File {
                    name: "large.bin".to_string(),
                    content: content_copy,
                    attr: FileAttributes::new_file(),
                };

                injector.inject_tree(&node).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_ntfs_large_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("ntfs_read_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
        let content = vec![0xAAu8; WRITE_SIZE];
        let node = FsNode::File {
            name: "large.bin".to_string(),
            content,
            attr: FileAttributes::new_file(),
        };

        injector.inject_tree(&node).unwrap();
        injector.flush().unwrap();
    }

    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));
    group.bench_function("read_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || disk_buf.clone(),
            |mut local_buf| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut resolver = NtfsResolver::new(&mut io, &meta);
                let data = resolver.read_file("/large.bin").unwrap();
                assert_eq!(data.len(), WRITE_SIZE);
            },
        );
    });

    group.finish();
}

fn bench_ntfs_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("ntfs_small_files");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const NUM_FILES: usize = 100;
    const FILE_SIZE: usize = 1024;

    let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();
    }

    let files: Vec<FsNode> = (0..NUM_FILES)
        .map(|i| FsNode::File {
            name: format!("file_{i}.txt"),
            content: vec![0xBB; FILE_SIZE],
            attr: FileAttributes::new_file(),
        })
        .collect();
    let tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children: files,
    };

    group.throughput(Throughput::Elements(NUM_FILES as u64));
    group.bench_function("create_100_small_files_mem", |b| {
        b.iter_with_setup(
            || disk_buf.clone(),
            |mut local_buf| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
                injector.inject_tree(&tree).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ntfs_format,
    bench_ntfs_large_write,
    bench_ntfs_large_read,
    bench_ntfs_small_files
);
criterion_main!(benches);
