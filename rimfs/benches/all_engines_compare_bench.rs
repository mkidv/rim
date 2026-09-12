// SPDX-License-Identifier: MIT

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use rimfs::{
    FsFormatter, FsTreeInjector, FsTreeResolver,
    core::resolver::{attr::FileAttributes, node::FsNode},
    exfat::*,
    ext::*,
    fat::*,
    iso::*,
    ntfs::*,
    tar::*,
    zip::*,
};

const SIZE_MB: u64 = 64;
const SIZE_BYTES: u64 = SIZE_MB * 1024 * 1024;
const WRITE_SIZE: usize = 10 * 1024 * 1024;
const NUM_FILES: usize = 100;
const FILE_SIZE: usize = 1024;

fn bench_format_all_engines(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare_format");
    group.throughput(Throughput::Bytes(SIZE_BYTES));

    group.bench_function(BenchmarkId::new("FAT12", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = FatMeta::new_fat12(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("FAT32", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("RimFAT", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH")).unwrap();
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("exFAT", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("EXT4", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = ExtMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            ExtFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("NTFS", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            NtfsFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("POSIX TAR", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = TarMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            TarFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("ZIP", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            ZipFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.bench_function(BenchmarkId::new("ISO 9660", "64MB"), |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE_BYTES as usize];
            let mut io = MemRimIO::new(&mut buf);
            let meta = IsoMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
            IsoFormatter::new(&mut io, &meta).format(false).unwrap();
        });
    });

    group.finish();
}

fn bench_write_large_all_engines(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare_write_10mb");
    group.throughput(Throughput::Bytes(WRITE_SIZE as u64));
    let content = vec![0xAAu8; WRITE_SIZE];

    // FAT32
    {
        let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        FatFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("FAT32", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::new_file("large.bin", content_copy);
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // exFAT
    {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        ExFatFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("exFAT", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = ExFatInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::new_file("large.bin", content_copy);
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // EXT4
    {
        let meta = ExtMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        ExtFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("EXT4", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = ExtInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::new_file("large.bin", content_copy);
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // NTFS
    {
        let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        NtfsFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("NTFS", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::new_file("large.bin", content_copy);
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // TAR
    {
        let meta = TarMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        TarFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("POSIX TAR", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = TarInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::new_file("large.bin", content_copy);
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // ZIP
    {
        let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        ZipFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("ZIP", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::new_file("large.bin", content_copy);
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // ISO 9660
    {
        let meta = IsoMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        IsoFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("ISO 9660", "10MB"), |b| {
            b.iter_with_setup(
                || (disk.clone(), content.clone()),
                |(mut local_disk, content_copy)| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
                    let mut node = FsNode::Container {
                        attr: FileAttributes::new_dir(),
                        children: vec![FsNode::new_file("large.bin", content_copy)],
                    };
                    injector.inject_tree(&mut node).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    group.finish();
}

fn bench_resolve_tree_all_engines(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare_resolve_tree_100_files");
    group.throughput(Throughput::Elements(NUM_FILES as u64));

    let make_tree = || {
        let files: Vec<FsNode> = (0..NUM_FILES)
            .map(|i| FsNode::new_file(format!("file_{i}.txt"), vec![0xBB; FILE_SIZE]))
            .collect();
        FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: files,
        }
    };

    // FAT32
    {
        let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            FatFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = FatInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("FAT32", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = FatResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    assert_eq!(node.counts().files, NUM_FILES);
                },
            );
        });
    }

    // exFAT
    {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            ExFatFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = ExFatInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("exFAT", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = ExFatResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    assert_eq!(node.counts().files, NUM_FILES);
                },
            );
        });
    }

    // EXT4
    {
        let meta = ExtMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            ExtFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = ExtInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("EXT4", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = ExtResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    assert_eq!(node.counts().files, NUM_FILES);
                },
            );
        });
    }

    // NTFS
    {
        let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            NtfsFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("NTFS", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = NtfsResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    // NTFS root index includes 13 system files ($MFT, $LogFile, etc.)
                    assert_eq!(node.counts().files, NUM_FILES + 13);
                },
            );
        });
    }

    // TAR
    {
        let meta = TarMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            TarFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = TarInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("POSIX TAR", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = TarResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    assert_eq!(node.counts().files, NUM_FILES);
                },
            );
        });
    }

    // ZIP
    {
        let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            ZipFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("ZIP", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = ZipResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    assert_eq!(node.counts().files, NUM_FILES);
                },
            );
        });
    }

    // ISO 9660
    {
        let meta = IsoMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        {
            let mut io = MemRimIO::new(&mut disk);
            IsoFormatter::new(&mut io, &meta).format(false).unwrap();
            let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
            injector.inject_tree(&mut make_tree()).unwrap();
            injector.flush().unwrap();
        }

        group.bench_function(BenchmarkId::new("ISO 9660", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut resolver = IsoResolver::new(&mut io, &meta);
                    let node = resolver.resolve_tree("/*").unwrap();
                    assert_eq!(node.counts().files, NUM_FILES);
                },
            );
        });
    }

    group.finish();
}

fn bench_inject_tree_all_engines(c: &mut Criterion) {
    let mut group = c.benchmark_group("compare_inject_tree_100_files");
    group.throughput(Throughput::Elements(NUM_FILES as u64));

    let make_tree = || {
        let files: Vec<FsNode> = (0..NUM_FILES)
            .map(|i| FsNode::new_file(format!("file_{i}.txt"), vec![0xBB; FILE_SIZE]))
            .collect();
        FsNode::Container {
            attr: FileAttributes::new_dir(),
            children: files,
        }
    };

    // FAT12
    {
        let meta = FatMeta::new_fat12(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        FatFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("FAT12", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // FAT32
    {
        let meta = FatMeta::new_fat32(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        FatFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("FAT32", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // RimFAT
    {
        let meta = FatMeta::new_rimfat(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        FatFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("RimFAT", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = FatInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // exFAT
    {
        let meta = ExFatMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        ExFatFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("exFAT", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = ExFatInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // EXT4
    {
        let meta = ExtMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        ExtFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("EXT4", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = ExtInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // NTFS
    {
        let meta = NtfsMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        NtfsFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("NTFS", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // POSIX TAR
    {
        let meta = TarMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        TarFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("POSIX TAR", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = TarInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // ZIP
    {
        let meta = ZipMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        ZipFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("ZIP", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = ZipInjector::new(&mut io, &meta).unwrap();
                    injector.inject_tree(&mut make_tree()).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    // ISO 9660
    {
        let meta = IsoMeta::new(SIZE_BYTES, Some("BENCH")).unwrap();
        let mut disk = vec![0u8; SIZE_BYTES as usize];
        IsoFormatter::new(&mut MemRimIO::new(&mut disk), &meta)
            .format(false)
            .unwrap();

        group.bench_function(BenchmarkId::new("ISO 9660", "100_files"), |b| {
            b.iter_with_setup(
                || disk.clone(),
                |mut local_disk| {
                    let mut io = MemRimIO::new(&mut local_disk);
                    let mut injector = IsoInjector::new(&mut io, &meta).unwrap();
                    let mut tree = FsNode::Container {
                        attr: FileAttributes::new_dir(),
                        children: (0..NUM_FILES)
                            .map(|i| {
                                FsNode::new_file(format!("file_{i}.txt"), vec![0xBB; FILE_SIZE])
                            })
                            .collect(),
                    };
                    injector.inject_tree(&mut tree).unwrap();
                    injector.flush().unwrap();
                },
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_format_all_engines,
    bench_write_large_all_engines,
    bench_inject_tree_all_engines,
    bench_resolve_tree_all_engines,
);
criterion_main!(benches);
