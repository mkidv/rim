use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs::ext4::*;

fn bench_ext4_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("ext4_format");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    group.throughput(Throughput::Bytes(SIZE_BYTES));
    group.bench_function("format_64mb_mem", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = Ext4Meta::new(SIZE_BYTES, Some("BENCH"));
            Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_disk", |b| {
        b.iter(|| {
            let mut file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let meta = Ext4Meta::new(SIZE_BYTES, Some("BENCH"));
            Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_ext4_large_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("ext4_write_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    // Setup FS
    let meta = Ext4Meta::new(SIZE_BYTES, Some("BENCH"));
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xAAu8; WRITE_SIZE];

    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));
    group.bench_function("write_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, mut content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut alloc = Ext4Allocator::new(&meta);
                let mut injector = Ext4Injector::new(&mut io, &mut alloc, &meta);

                let len = content_copy.len() as u64;
                let mut content_io = MemRimIO::new(&mut content_copy);

                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();
                injector
                    .write_file(
                        "bigfile.bin",
                        &mut content_io,
                        len,
                        &FileAttributes::default(),
                    )
                    .unwrap();
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
                Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, mut content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut alloc = Ext4Allocator::new(&meta);
                let mut injector = Ext4Injector::new(&mut io, &mut alloc, &meta);

                let len = content_copy.len() as u64;
                let mut content_io = MemRimIO::new(&mut content_copy);

                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();
                injector
                    .write_file(
                        "bigfile.bin",
                        &mut content_io,
                        len,
                        &FileAttributes::default(),
                    )
                    .unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_ext4_large_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("ext4_read_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    // MEM SETUP
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    let meta = Ext4Meta::new(SIZE_BYTES, Some("BENCH"));
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
        let mut alloc = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut alloc, &meta);
        injector
            .set_root_context(&FsNode::new_container(vec![]))
            .unwrap();
        let mut content = vec![0xAAu8; WRITE_SIZE];
        let mut content_io = MemRimIO::new(&mut content);
        injector
            .write_file(
                "bigfile.bin",
                &mut content_io,
                WRITE_SIZE as u64,
                &FileAttributes::default(),
            )
            .unwrap();
        injector.flush().unwrap();
    }

    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));
    group.bench_function("read_10mb_contiguous_mem", |b| {
        b.iter(|| {
            let mut io = MemRimIO::new(&mut disk_buf);
            let mut resolver = Ext4Resolver::new(&mut io, &meta);
            let data = resolver.read_file("/bigfile.bin").unwrap();
            assert_eq!(data.len(), WRITE_SIZE);
        });
    });

    // DISK SETUP
    let mut file = tempfile::tempfile().unwrap();
    file.set_len(SIZE_BYTES).unwrap();
    {
        let mut io = StdRimIO::new(&mut file);
        Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
        let mut alloc = Ext4Allocator::new(&meta);
        let mut injector = Ext4Injector::new(&mut io, &mut alloc, &meta);
        injector
            .set_root_context(&FsNode::new_container(vec![]))
            .unwrap();
        let mut content = vec![0xAAu8; WRITE_SIZE];
        let mut content_io = MemRimIO::new(&mut content);
        injector
            .write_file(
                "bigfile.bin",
                &mut content_io,
                WRITE_SIZE as u64,
                &FileAttributes::default(),
            )
            .unwrap();
        injector.flush().unwrap();
    }

    group.bench_function("read_10mb_contiguous_disk", |b| {
        b.iter(|| {
            let mut io = StdRimIO::new(&mut file);
            let mut resolver = Ext4Resolver::new(&mut io, &meta);
            let data = resolver.read_file("/bigfile.bin").unwrap();
            assert_eq!(data.len(), WRITE_SIZE);
        });
    });

    group.finish();
}

fn bench_ext4_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("ext4_small_files");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const NUM_FILES: usize = 100;
    const FILE_SIZE: usize = 100;

    let meta = Ext4Meta::new(SIZE_BYTES, Some("BENCH"));
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xBBu8; FILE_SIZE];

    group.bench_function("create_100_small_files_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, mut content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut alloc = Ext4Allocator::new(&meta);
                let mut injector = Ext4Injector::new(&mut io, &mut alloc, &meta);

                let len = content_copy.len() as u64;
                let mut content_io = MemRimIO::new(&mut content_copy);

                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();

                for i in 0..NUM_FILES {
                    let name = format!("file{i}.txt");
                    injector
                        .write_file(&name, &mut content_io, len, &FileAttributes::default())
                        .unwrap();
                    // No seek logic needed
                }
                injector.flush().unwrap();
            },
        );
    });

    group.bench_function("create_100_small_files_disk", |b| {
        b.iter_with_setup(
            || {
                let mut file = tempfile::tempfile().unwrap();
                file.set_len(SIZE_BYTES).unwrap();
                let mut io = StdRimIO::new(&mut file);
                Ext4Formatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, mut content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut alloc = Ext4Allocator::new(&meta);
                let mut injector = Ext4Injector::new(&mut io, &mut alloc, &meta);

                let len = content_copy.len() as u64;
                let mut content_io = MemRimIO::new(&mut content_copy);

                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();

                for i in 0..NUM_FILES {
                    let name = format!("file{i}.txt");
                    injector
                        .write_file(&name, &mut content_io, len, &FileAttributes::default())
                        .unwrap();
                    // No seek logic needed
                }
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ext4_format,
    bench_ext4_large_write,
    bench_ext4_large_read,
    bench_ext4_small_files
);
criterion_main!(benches);
