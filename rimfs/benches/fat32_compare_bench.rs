use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use fatfs::{FileSystem, FormatVolumeOptions, FsOptions, format_volume};
use fscommon::BufStream;
use rimfs::fat::*;
use std::hint::black_box;
use std::io::{Cursor, Read, Write};

const SIZE_MB: u64 = 64;
const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
const WRITE_SIZE: usize = 10 * 1024 * 1024;
const SMALL_FILE_SIZE: usize = 1024;
const SMALL_FILES_COUNT: usize = 100;
const RANDOM_IO_BLOCK: usize = 4096;
const RANDOM_IO_OPS: usize = 256;
const LOOKUP_FILES: usize = 240;
const DEEP_NEST_LEVELS: usize = 32;

fn init_rimfs_disk() -> (Vec<u8>, FatMeta) {
    let mut disk = vec![0u8; SIZE_BYTES as usize];
    let mut io = MemRimIO::new(&mut disk);
    let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
    FatFormatter::new(&mut io, &meta).format(false).unwrap();
    (disk, meta)
}

fn init_fatfs_disk() -> Cursor<Vec<u8>> {
    let mut disk = Cursor::new(vec![0u8; SIZE_BYTES as usize]);
    let opts = FormatVolumeOptions::new().bytes_per_sector(512);
    format_volume(&mut disk, opts).unwrap();
    disk
}

fn bench_format_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_format_compare");
    group.throughput(Throughput::Bytes(SIZE_BYTES));

    group.bench_function(BenchmarkId::new("rimfs", "format_64mb_mem"), |b| {
        b.iter(|| {
            let mut disk = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut disk);
            let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("fatfs", "format_64mb_mem"), |b| {
        b.iter(|| {
            let mut disk = Cursor::new(vec![0u8; SIZE_BYTES as usize]);
            let opts = FormatVolumeOptions::new().bytes_per_sector(512);
            format_volume(&mut disk, opts).unwrap();
        });
    });

    group.finish();
}

fn bench_write_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_write_compare");
    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));

    let content = vec![0xAAu8; WRITE_SIZE];

    group.bench_function(BenchmarkId::new("rimfs", "write_10mb_mem"), |b| {
        b.iter_with_setup(
            || {
                let (disk, meta) = init_rimfs_disk();
                (disk, meta, content.clone())
            },
            |(mut disk, meta, mut content_copy)| {
                let mut io = MemRimIO::new(&mut disk);
                let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();

                let len = content_copy.len() as u64;
                let mut content_io = MemRimIO::new(&mut content_copy);
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

    group.bench_function(BenchmarkId::new("fatfs", "write_10mb_mem"), |b| {
        b.iter_with_setup(
            || {
                let disk = init_fatfs_disk();
                (disk, content.clone())
            },
            |(disk, content_copy)| {
                let mut stream = BufStream::new(disk);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();
                let mut f = root.create_file("bigfile.bin").unwrap();
                f.write_all(&content_copy).unwrap();
                f.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_read_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_read_compare");
    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));

    let content = vec![0xAAu8; WRITE_SIZE];

    let (mut rim_disk, rim_meta) = init_rimfs_disk();
    {
        let mut io = MemRimIO::new(&mut rim_disk);
        let mut injector = FatInjector::new(&mut io, &rim_meta).unwrap();
        injector
            .set_root_context(&FsNode::new_container(vec![]))
            .unwrap();
        let mut src = content.clone();
        let mut content_io = MemRimIO::new(&mut src);
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

    let mut fat_disk = init_fatfs_disk();
    {
        let mut stream = BufStream::new(&mut fat_disk);
        let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
        let root = fs.root_dir();
        let mut f = root.create_file("bigfile.bin").unwrap();
        f.write_all(&content).unwrap();
        f.flush().unwrap();
    }

    group.bench_function(BenchmarkId::new("rimfs", "read_10mb_mem"), |b| {
        b.iter_with_setup(
            || rim_disk.clone(),
            |mut local_disk| {
                let mut io = MemRimIO::new(&mut local_disk);
                let mut resolver = FatResolver::new(&mut io, &rim_meta);
                let data = resolver.read_file("/bigfile.bin").unwrap();
                assert_eq!(data.len(), WRITE_SIZE);
            },
        );
    });

    group.bench_function(BenchmarkId::new("fatfs", "read_10mb_mem"), |b| {
        b.iter_with_setup(
            || fat_disk.get_ref().clone(),
            |local_disk| {
                let cursor = Cursor::new(local_disk);
                let mut stream = BufStream::new(cursor);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();
                let mut f = root.open_file("bigfile.bin").unwrap();
                let mut out = Vec::with_capacity(WRITE_SIZE);
                f.read_to_end(&mut out).unwrap();
                assert_eq!(out.len(), WRITE_SIZE);
            },
        );
    });

    group.finish();
}

fn bench_small_files_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_small_files_compare");
    group.throughput(Throughput::Elements(SMALL_FILES_COUNT as u64));

    let content = vec![0xBBu8; SMALL_FILE_SIZE];

    group.bench_function(BenchmarkId::new("rimfs", "create_100x1kb_mem"), |b| {
        b.iter_with_setup(
            || {
                let (disk, meta) = init_rimfs_disk();
                (disk, meta, content.clone())
            },
            |(mut disk, meta, mut content_copy)| {
                let mut io = MemRimIO::new(&mut disk);
                let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();

                let len = content_copy.len() as u64;
                for i in 0..SMALL_FILES_COUNT {
                    let mut content_io = MemRimIO::new(&mut content_copy);
                    let name = format!("file_{i:03}.txt");
                    injector
                        .write_file(&name, &mut content_io, len, &FileAttributes::default())
                        .unwrap();
                }
                injector.flush().unwrap();
            },
        );
    });

    group.bench_function(BenchmarkId::new("fatfs", "create_100x1kb_mem"), |b| {
        b.iter_with_setup(
            || {
                let disk = init_fatfs_disk();
                (disk, content.clone())
            },
            |(disk, content_copy)| {
                let mut stream = BufStream::new(disk);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();

                for i in 0..SMALL_FILES_COUNT {
                    let name = format!("file_{i:03}.txt");
                    let mut f = root.create_file(&name).unwrap();
                    f.write_all(&content_copy).unwrap();
                    f.flush().unwrap();
                }
            },
        );
    });

    group.finish();
}

fn bench_overwrite_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_overwrite_compare");
    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));

    let original = vec![0x11u8; WRITE_SIZE];
    let updated = vec![0x22u8; WRITE_SIZE];

    group.bench_function(BenchmarkId::new("rimfs", "overwrite_10mb_mem"), |b| {
        b.iter_with_setup(
            || {
                let (mut disk, meta) = init_rimfs_disk();
                {
                    let mut io = MemRimIO::new(&mut disk);
                    let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                    injector
                        .set_root_context(&FsNode::new_container(vec![]))
                        .unwrap();
                    let mut src = original.clone();
                    let mut src_io = MemRimIO::new(&mut src);
                    injector
                        .write_file(
                            "bigfile.bin",
                            &mut src_io,
                            WRITE_SIZE as u64,
                            &FileAttributes::default(),
                        )
                        .unwrap();
                    injector.flush().unwrap();
                }
                (disk, meta, updated.clone())
            },
            |(mut disk, meta, mut updated_copy)| {
                let mut io = MemRimIO::new(&mut disk);
                let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                injector
                    .set_root_context(&FsNode::new_container(vec![]))
                    .unwrap();
                let mut src_io = MemRimIO::new(&mut updated_copy);
                injector
                    .write_file(
                        "bigfile.bin",
                        &mut src_io,
                        WRITE_SIZE as u64,
                        &FileAttributes::default(),
                    )
                    .unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.bench_function(BenchmarkId::new("fatfs", "overwrite_10mb_mem"), |b| {
        b.iter_with_setup(
            || {
                let mut disk = init_fatfs_disk();
                {
                    let mut stream = BufStream::new(&mut disk);
                    let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                    let root = fs.root_dir();
                    let mut f = root.create_file("bigfile.bin").unwrap();
                    f.write_all(&original).unwrap();
                    f.flush().unwrap();
                }
                disk.set_position(0);
                (disk, updated.clone())
            },
            |(mut disk, updated_copy)| {
                disk.set_position(0);
                let mut stream = BufStream::new(&mut disk);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();
                let mut f = root.create_file("bigfile.bin").unwrap();
                f.write_all(&updated_copy).unwrap();
                f.flush().unwrap();
            },
        );
    });

    group.finish();
}

fn bench_random_read_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_random_read_compare");
    group.throughput(Throughput::Bytes((RANDOM_IO_BLOCK * RANDOM_IO_OPS) as u64));

    let seed_data = vec![0xABu8; WRITE_SIZE];

    let (mut rim_disk, rim_meta) = init_rimfs_disk();
    {
        let mut io = MemRimIO::new(&mut rim_disk);
        let mut injector = FatInjector::new(&mut io, &rim_meta).unwrap();
        injector
            .set_root_context(&FsNode::new_container(vec![]))
            .unwrap();
        let mut src = seed_data.clone();
        let mut src_io = MemRimIO::new(&mut src);
        injector
            .write_file(
                "rand.bin",
                &mut src_io,
                WRITE_SIZE as u64,
                &FileAttributes::default(),
            )
            .unwrap();
        injector.flush().unwrap();
    }

    let mut fat_disk = init_fatfs_disk();
    {
        let mut stream = BufStream::new(&mut fat_disk);
        let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
        let root = fs.root_dir();
        let mut f = root.create_file("rand.bin").unwrap();
        f.write_all(&seed_data).unwrap();
        f.flush().unwrap();
    }

    let offsets: Vec<usize> = (0..RANDOM_IO_OPS)
        .map(|i| (i * 1543) % (WRITE_SIZE - RANDOM_IO_BLOCK))
        .collect();

    group.bench_function(BenchmarkId::new("rimfs", "random_256x4k_mem"), |b| {
        b.iter_with_setup(
            || rim_disk.clone(),
            |mut local_disk| {
                let mut io = MemRimIO::new(&mut local_disk);
                let mut resolver = FatResolver::new(&mut io, &rim_meta);
                let data = resolver.read_file("/rand.bin").unwrap();
                let mut sink = 0u8;
                for &off in &offsets {
                    sink ^= data[off];
                }
                black_box(sink);
            },
        );
    });

    group.bench_function(BenchmarkId::new("fatfs", "random_256x4k_mem"), |b| {
        b.iter_with_setup(
            || fat_disk.get_ref().clone(),
            |local_disk| {
                let cursor = Cursor::new(local_disk);
                let mut stream = BufStream::new(cursor);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();
                let mut f = root.open_file("rand.bin").unwrap();
                let mut data = Vec::with_capacity(WRITE_SIZE);
                f.read_to_end(&mut data).unwrap();
                let mut sink = 0u8;
                for &off in &offsets {
                    sink ^= data[off];
                }
                black_box(sink);
            },
        );
    });

    group.finish();
}

fn bench_worst_lookup_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_worst_lookup_compare");
    group.throughput(Throughput::Elements(1));

    let payload = vec![0x5Au8; 64];
    let target = format!("file_{:04}.txt", LOOKUP_FILES - 1);

    let (mut rim_disk, rim_meta) = init_rimfs_disk();
    {
        let mut io = MemRimIO::new(&mut rim_disk);
        let mut injector = FatInjector::new(&mut io, &rim_meta).unwrap();
        injector
            .set_root_context(&FsNode::new_container(vec![]))
            .unwrap();
        for i in 0..LOOKUP_FILES {
            let mut src = payload.clone();
            let mut src_io = MemRimIO::new(&mut src);
            let name = format!("file_{i:04}.txt");
            injector
                .write_file(
                    &name,
                    &mut src_io,
                    payload.len() as u64,
                    &FileAttributes::default(),
                )
                .unwrap();
        }
        injector.flush().unwrap();
    }

    let mut fat_disk = init_fatfs_disk();
    {
        let mut stream = BufStream::new(&mut fat_disk);
        let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
        let root = fs.root_dir();
        for i in 0..LOOKUP_FILES {
            let name = format!("file_{i:04}.txt");
            let mut f = root.create_file(&name).unwrap();
            f.write_all(&payload).unwrap();
            f.flush().unwrap();
        }
    }

    group.bench_function(BenchmarkId::new("rimfs", "lookup_last_in_240"), |b| {
        b.iter_with_setup(
            || rim_disk.clone(),
            |mut local_disk| {
                let mut io = MemRimIO::new(&mut local_disk);
                let mut resolver = FatResolver::new(&mut io, &rim_meta);
                let data = resolver.read_file(&format!("/{target}")).unwrap();
                black_box(data.len());
            },
        );
    });

    group.bench_function(BenchmarkId::new("fatfs", "lookup_last_in_240"), |b| {
        b.iter_with_setup(
            || fat_disk.get_ref().clone(),
            |local_disk| {
                let cursor = Cursor::new(local_disk);
                let mut stream = BufStream::new(cursor);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();
                let mut f = root.open_file(&target).unwrap();
                let mut out = Vec::new();
                f.read_to_end(&mut out).unwrap();
                black_box(out.len());
            },
        );
    });

    group.finish();
}

fn bench_deep_path_compare(c: &mut Criterion) {
    let mut group = c.benchmark_group("fat32_deep_path_compare");
    group.throughput(Throughput::Elements(1));

    let payload = vec![0x33u8; 256];

    let mut rim_path = String::from("/");
    for i in 0..DEEP_NEST_LEVELS {
        if i > 0 {
            rim_path.push('/');
        }
        rim_path.push_str(&format!("d{i:02}"));
    }
    rim_path.push_str("/deep.bin");

    let mut node = FsNode::new_file("deep.bin", payload.clone());
    for i in (0..DEEP_NEST_LEVELS).rev() {
        let mut dir = FsNode::new_dir(format!("d{i:02}"));
        if let FsNode::Dir { children, .. } = &mut dir {
            children.push(node);
        }
        node = dir;
    }
    let root = FsNode::new_container(vec![node]);

    let (mut rim_disk, rim_meta) = init_rimfs_disk();
    {
        let mut io = MemRimIO::new(&mut rim_disk);
        let mut injector = FatInjector::new(&mut io, &rim_meta).unwrap();
        injector.inject_tree(&root).unwrap();
        injector.flush().unwrap();
    }

    let mut fat_disk = init_fatfs_disk();
    {
        let mut stream = BufStream::new(&mut fat_disk);
        let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
        let mut dir = fs.root_dir();
        for i in 0..DEEP_NEST_LEVELS {
            let name = format!("d{i:02}");
            dir = dir.create_dir(&name).unwrap();
        }
        let mut f = dir.create_file("deep.bin").unwrap();
        f.write_all(&payload).unwrap();
        f.flush().unwrap();
    }

    group.bench_function(BenchmarkId::new("rimfs", "read_depth32_path"), |b| {
        b.iter_with_setup(
            || rim_disk.clone(),
            |mut local_disk| {
                let mut io = MemRimIO::new(&mut local_disk);
                let mut resolver = FatResolver::new(&mut io, &rim_meta);
                let data = resolver.read_file(&rim_path).unwrap();
                black_box(data.len());
            },
        );
    });

    group.bench_function(BenchmarkId::new("fatfs", "read_depth32_path"), |b| {
        b.iter_with_setup(
            || fat_disk.get_ref().clone(),
            |local_disk| {
                let cursor = Cursor::new(local_disk);
                let mut stream = BufStream::new(cursor);
                let fs = FileSystem::new(&mut stream, FsOptions::new()).unwrap();
                let root = fs.root_dir();
                let mut f = root.open_file(&rim_path[1..]).unwrap();
                let mut out = Vec::new();
                f.read_to_end(&mut out).unwrap();
                black_box(out.len());
            },
        );
    });

    group.finish();
}
criterion_group!(
    compare_benches,
    bench_format_compare,
    bench_write_compare,
    bench_read_compare,
    bench_small_files_compare,
    bench_overwrite_compare,
    bench_random_read_compare,
    bench_worst_lookup_compare,
    bench_deep_path_compare
);
criterion_main!(compare_benches);
