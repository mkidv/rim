use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs::exfat::*;

fn bench_exfat_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("exfat_format");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    group.throughput(Throughput::Bytes(SIZE_BYTES));
    group.bench_function("format_64mb_mem", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_disk", |b| {
        b.iter(|| {
            let mut file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_exfat_large_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("exfat_write_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    // Write 10MB
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    // Setup FS once
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xAAu8; WRITE_SIZE];

    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));
    group.bench_function("write_10mb_contiguous_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, mut content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut alloc = ExFatAllocator::new(&meta);
                let mut injector = ExFatInjector::new(&mut io, &mut alloc, &meta).unwrap();

                let len = content_copy.len() as u64;
                let mut content_io = MemRimIO::new(&mut content_copy);

                // We simulate writing a file at root
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
                ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, mut content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut alloc = ExFatAllocator::new(&meta);
                let mut injector = ExFatInjector::new(&mut io, &mut alloc, &meta).unwrap();

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

fn bench_exfat_large_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("exfat_read_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    // Setup FS with file ONCE (for memory)
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut alloc = ExFatAllocator::new(&meta);
        let mut injector = ExFatInjector::new(&mut io, &mut alloc, &meta).unwrap();
        injector
            .set_root_context(&FsNode::new_container(vec![]))
            .unwrap();
        // Write a 10MB file
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
            let mut resolver = ExFatResolver::new(&mut io, &meta);
            let data = resolver.read_file("/bigfile.bin").unwrap();
            assert_eq!(data.len(), WRITE_SIZE);
        });
    });

    // DISK SETUP involves re-populating the file every iteration OR creating a reusable file
    // Create a populated file once for reading (for disk)
    let mut file = tempfile::tempfile().unwrap();
    file.set_len(SIZE_BYTES).unwrap();
    {
        let mut io = StdRimIO::new(&mut file);
        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut alloc = ExFatAllocator::new(&meta);
        let mut injector = ExFatInjector::new(&mut io, &mut alloc, &meta).unwrap();
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
            // StdRimIO takes &mut T, so we need to ensure we can seek back or re-create wrapper
            // StdRimIO doesn't own the file.
            // Problem: StdRimIO constructor resets offset to 0 if new() is used? No, it just takes reference.
            // We need to ensure we don't mess up file position if resolver relies on it?
            // Resolver uses absolute reads, so it's fine.
            let mut io = StdRimIO::new(&mut file);
            let mut resolver = ExFatResolver::new(&mut io, &meta);
            let data = resolver.read_file("/bigfile.bin").unwrap();
            assert_eq!(data.len(), WRITE_SIZE);
        });
    });

    group.finish();
}

fn bench_exfat_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("exfat_small_files");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const NUM_FILES: usize = 100;
    const FILE_SIZE: usize = 100;

    let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xBBu8; FILE_SIZE];

    group.bench_function("create_100_small_files_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, mut content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut alloc = ExFatAllocator::new(&meta);
                let mut injector = ExFatInjector::new(&mut io, &mut alloc, &meta).unwrap();

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
                    // We can't seek easily with MemRimIO as it lacks Seek trait implementation
                    // But write_file resets offset to 0 for source!
                    // So we don't need to seek. It will read from 0 every time.
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
                ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, mut content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut alloc = ExFatAllocator::new(&meta);
                let mut injector = ExFatInjector::new(&mut io, &mut alloc, &meta).unwrap();

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
                    // No seek needed
                }
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_exfat_format,
    bench_exfat_large_write,
    bench_exfat_large_read,
    bench_exfat_small_files
);
criterion_main!(benches);
