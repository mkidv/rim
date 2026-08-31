use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs_fat::prelude::*;

fn bench_fat_format(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat_format");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;

    group.throughput(Throughput::Bytes(SIZE_BYTES));
    group.bench_function("format_64mb_mem", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_disk", |b| {
        b.iter(|| {
            let mut file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = StdRimIO::new(&mut file);
            let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function("format_64mb_mmap", |b| {
        b.iter(|| {
            let file = tempfile::tempfile().unwrap();
            file.set_len(SIZE_BYTES).unwrap();
            let mut io = MmapRimIO::new(file).unwrap();
            let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_fat_large_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat_write_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    // Setup FS
    let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
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
            |(mut local_buf, mut content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");

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
                FatFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, mut content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");

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

    group.bench_function("write_10mb_contiguous_mmap", |b| {
        b.iter_with_setup(
            || {
                let file = tempfile::tempfile().unwrap();
                file.set_len(SIZE_BYTES).unwrap();
                let mut io = MmapRimIO::new(file.try_clone().unwrap()).unwrap();
                let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
                FatFormatter::new(&mut io, &meta).format(false).unwrap();
                // Return file for bench. MmapRimIO consumed clone, but we need fresh file for next iteration?
                // Actually MmapRimIO takes ownership. So we need to create a new file each iter logic if possible.
                // But setup returns (file, content).
                // Wait. We need to format it first.
                // MmapRimIO map takes ownership of file.
                // If we want to pass file to bench, we need to clone it or reopen?
                // Tempfile is unlinked. Try_clone works on Windows? Yes.
                // But MmapRimIO::new consumes File.
                // So we format using one handle, then return another handle?
                (file, content.clone())
            },
            |(file, mut content_copy)| {
                let mut io = MmapRimIO::new(file).unwrap();
                let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");

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

fn bench_fat_large_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat_read_large");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const WRITE_SIZE: usize = 10 * 1024 * 1024;

    // MEM SETUP
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");
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
            let mut resolver = FatResolver::new(&mut io, &meta);
            let data = resolver.read_file("/bigfile.bin").unwrap();
            assert_eq!(data.len(), WRITE_SIZE);
        });
    });

    // DISK SETUP
    let mut file = tempfile::tempfile().unwrap();
    file.set_len(SIZE_BYTES).unwrap();
    {
        let mut io = StdRimIO::new(&mut file);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");
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
            let mut resolver = FatResolver::new(&mut io, &meta);
            let data = resolver.read_file("/bigfile.bin").unwrap();
            assert_eq!(data.len(), WRITE_SIZE);
        });
    });

    // MMAP SETUP
    let file_mmap = tempfile::tempfile().unwrap();
    file_mmap.set_len(SIZE_BYTES).unwrap();
    {
        let mut io = MmapRimIO::new(file_mmap.try_clone().unwrap()).unwrap();
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
        let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");
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

    group.bench_function("read_10mb_contiguous_mmap", |b| {
        b.iter_with_setup(
            || MmapRimIO::new(file_mmap.try_clone().unwrap()).unwrap(),
            |mut io| {
                let mut resolver = FatResolver::new(&mut io, &meta);
                let data = resolver.read_file("/bigfile.bin").unwrap();
                assert_eq!(data.len(), WRITE_SIZE);
            },
        );
    });

    group.finish();
}

fn bench_fat_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat_small_files");
    const SIZE_MB: u64 = 64;
    const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
    const NUM_FILES: usize = 100;
    const FILE_SIZE: usize = 100;

    let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
    let mut disk_buf = vec![0u8; SIZE_BYTES as usize];
    {
        let mut io = MemRimIO::new(&mut disk_buf);
        FatFormatter::new(&mut io, &meta).format(false).unwrap();
    }

    let content = vec![0xBBu8; FILE_SIZE];

    group.bench_function("create_100_small_files_mem", |b| {
        b.iter_with_setup(
            || (disk_buf.clone(), content.clone()),
            |(mut local_buf, mut content_copy)| {
                let mut io = MemRimIO::new(&mut local_buf);
                let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");

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
                FatFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(mut file, mut content_copy)| {
                let mut io = StdRimIO::new(&mut file);
                let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");

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
                }
                injector.flush().unwrap();
            },
        );
    });

    group.bench_function("create_100_small_files_mmap", |b| {
        b.iter_with_setup(
            || {
                let file = tempfile::tempfile().unwrap();
                file.set_len(SIZE_BYTES).unwrap();
                let mut io = MmapRimIO::new(file.try_clone().unwrap()).unwrap();
                FatFormatter::new(&mut io, &meta).format(false).unwrap();
                (file, content.clone())
            },
            |(file, mut content_copy)| {
                let mut io = MmapRimIO::new(file).unwrap();
                let mut injector = FatInjector::new(&mut io, &meta).expect("injector failed");

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
                }
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_fat_format,
    bench_fat_large_write,
    bench_fat_large_read,
    bench_fat_small_files
);
criterion_main!(benches);
