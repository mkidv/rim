// SPDX-License-Identifier: MIT

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rimfs_ntfs::prelude::*;

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
            let mut io = unsafe { MmapRimIO::new(file) }.unwrap();
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

                let mut node = FsNode::new_file("large.bin", content_copy);

                injector.inject_tree(&mut node).unwrap();
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
                let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
                injector.inject_tree(&mut local_tree).unwrap();
                injector.flush().unwrap();
            },
        );
    });

    group.finish();
}

// Isolate attribute-name lookup from disk setup and filesystem formatting.
fn bench_ntfs_attribute_name(c: &mut Criterion) {
    use rimfs_ntfs::{types::AttributeHeader, view::attr_view::AttrRef};
    use zerocopy::FromBytes;
    let mut group = c.benchmark_group("ntfs_attribute_name");
    for (label, name, target) in [
        (
            "ascii_match",
            "MyAlternateDataStream",
            "MYALTERNATEDATASTREAM",
        ),
        ("unicode_match", "metadata_\u{1f600}", "METADATA_\u{1f600}"),
        ("miss", "MyAlternateDataStream", "OtherStream"),
    ] {
        let units: Vec<u16> = name.encode_utf16().collect();
        let mut bytes = vec![0u8; 17 + units.len() * 2];
        bytes[9] = units.len() as u8;
        bytes[10..12].copy_from_slice(&17u16.to_le_bytes());
        for (slot, unit) in bytes[17..].chunks_exact_mut(2).zip(units) {
            slot.copy_from_slice(&unit.to_le_bytes());
        }
        let header = AttributeHeader::ref_from_prefix(&bytes).unwrap().0;
        let attr = AttrRef {
            raw: &bytes,
            header,
        };
        group.bench_function(format!("{label}/previous_allocating"), |b| {
            b.iter(|| {
                let attr = std::hint::black_box(attr);
                let start = attr.header.name_offset.get() as usize;
                let end = start + attr.header.name_length as usize * 2;
                let units: Vec<u16> = attr.raw[start..end]
                    .chunks_exact(2)
                    .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
                    .collect();
                std::hint::black_box(
                    String::from_utf16(&units).ok().is_some_and(|name| {
                        name.eq_ignore_ascii_case(std::hint::black_box(target))
                    }),
                )
            })
        });
        group.bench_function(format!("{label}/borrowed"), |b| {
            b.iter(|| {
                std::hint::black_box(
                    std::hint::black_box(attr)
                        .name_eq_ignore_ascii_case(std::hint::black_box(target)),
                )
            })
        });
    }
    group.finish();
}

fn bench_metadata_buffers(c: &mut Criterion) {
    use rimfs_ntfs::types::{
        MftRecordBuilder, NtfsMftRecord, SECURITY_DESCRIPTOR_ROOT, calculate_security_hash,
    };
    let mut group = c.benchmark_group("ntfs_metadata_buffers");
    group.bench_function("security_hash_allocating", |b| {
        b.iter(|| {
            calculate_security_hash(&std::hint::black_box(SECURITY_DESCRIPTOR_ROOT).to_bytes())
        })
    });
    group.bench_function("security_hash_borrowed", |b| {
        b.iter(|| {
            std::hint::black_box(SECURITY_DESCRIPTOR_ROOT)
                .hash()
                .unwrap()
        })
    });
    let meta = NtfsMeta::new(64 * 1024 * 1024, None).unwrap();
    let record = NtfsMftRecord::new(42, false, true);
    let mut buffer = [0u8; 1024];
    group.bench_function("mft_generic_finalize", |b| {
        b.iter(|| {
            buffer.fill(0);
            let mut io = MemRimIO::new(&mut buffer);
            let mut builder = MftRecordBuilder::new(&mut io, &meta, 0);
            builder.write_header(record.header).unwrap();
            builder.finalize().unwrap();
            std::hint::black_box(&buffer);
        })
    });
    group.bench_function("mft_direct_finalize", |b| {
        b.iter(|| {
            record.serialize_into(&mut buffer, &meta).unwrap();
            std::hint::black_box(&buffer);
        })
    });
    let mut paths = rimfs_ntfs::core::resolver::PathIndex::new();
    for i in 0..10000 {
        paths.insert(&format!("directory/file{i:05}"), i);
    }
    group.bench_function("path_children_owned", |b| {
        b.iter(|| {
            std::hint::black_box(
                paths
                    .children("directory")
                    .unwrap()
                    .iter()
                    .map(|name| std::hint::black_box(name.as_str()).len())
                    .sum::<usize>(),
            )
        })
    });
    group.bench_function("path_children_borrowed", |b| {
        b.iter(|| {
            std::hint::black_box(
                paths
                    .children_iter("directory")
                    .unwrap()
                    .map(|name| std::hint::black_box(name).len())
                    .sum::<usize>(),
            )
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_ntfs_format,
    bench_ntfs_large_write,
    bench_ntfs_large_read,
    bench_ntfs_small_files,
    bench_ntfs_attribute_name,
    bench_metadata_buffers
);
criterion_main!(benches);
