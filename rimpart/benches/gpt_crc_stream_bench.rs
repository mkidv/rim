// cargo bench -p rimpart --features std,mem
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use rimpart::gpt_stream::GptStreamReader;
use rimpart::gpt_stream::GptStreamWriter;
use zerocopy::IntoBytes;

use rimio::prelude::MemRimIO;
use rimpart::gpt;
use rimpart::mbr;

criterion_group!(
    benches,
    bench_crc,
    bench_read_stream_vs_alloc,
    bench_write_stream_vs_alloc
);
criterion_main!(benches);

fn make_guid(i: usize) -> [u8; 16] {
    let mut g = [0u8; 16];
    let n = i as u128;
    g.copy_from_slice(&n.to_le_bytes()); // 16 octets LE
    g
}

fn make_header_and_entries(
    sector: u64,
    total: u64,
    n: usize,
) -> (gpt::GptHeader, Vec<gpt::GptEntry>) {
    // Header with consistent bounds
    let entry_sz = size_of::<gpt::GptEntry>() as u32;
    let mut hdr =
        gpt::GptHeader::new_with_table(sector, total, make_guid(0), n as u32, entry_sz).unwrap();

    // Prepare N alloc requests (type, unique_guid, len_sectors, attrs, name)
    //    Ici on prend 1024 secteurs/part (~512 KiB à 512B) pour le bench.
    let len_sectors = 1024u64;
    let attrs = 0u64;

    // Génère les tuples attendus par make_aligned_entries (réfs requises)
    // On stocke les GUID et noms pour fournir des références stables
    let uids: Vec<[u8; 16]> = (0..n).map(|i| make_guid(i + 1)).collect();
    let names: Vec<String> = (0..n).map(|i| format!("p{i}")).collect();

    let reqs = (0..n).map(|i| {
        (
            &rimpart::guids::GPT_PARTITION_TYPE_DATA, // type_guid
            &uids[i],                                 // unique_guid
            len_sectors,                              // longueur en secteurs
            attrs,                                    // attributes
            &*names[i],                               // name &str
        )
    });

    // Let gpt::make_aligned_entries do the 1MiB-Align + bounds placement
    let entries = gpt::make_aligned_entries_fit(&hdr, sector, reqs).unwrap();

    hdr.compute_crc32(&entries);
    (hdr, entries)
}

fn bench_crc(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpt_crc");
    // tailles représentatives
    for &n in &[128usize, 1024, 4096] {
        let sector = 512u64;
        let total = 200_000u64; // large pour l'alignement
        let (hdr, entries) = make_header_and_entries(sector, total, n);

        // Simule la "region" (table entries complète, num_entries slots)
        let es = hdr.entry_size as usize;
        let ne = hdr.num_entries as usize;
        let base = core::mem::size_of::<gpt::GptEntry>();
        let mut region = vec![0u8; es * ne];
        for (i, p) in entries.iter().enumerate() {
            let head = p.as_bytes();
            let dst = &mut region[i * es..i * es + base];
            dst.copy_from_slice(&head[..base]);
            // tail déjà zéro
        }

        group.bench_with_input(BenchmarkId::new("iter_entries_heads", n), &n, |b, &_n| {
            b.iter(|| {
                let crc = gpt::compute_entries_crc32_from_iter(
                    entries.iter().map(|e| {
                        let mut buf = [0u8; core::mem::size_of::<gpt::GptEntry>()];
                        buf.copy_from_slice(e.as_bytes());
                        buf
                    }),
                    &hdr,
                );
                std::hint::black_box(crc)
            });
        });

        group.bench_with_input(BenchmarkId::new("region_chunks", n), &n, |b, &_n| {
            b.iter(|| {
                let crc = gpt::compute_entries_crc32_from_iter(
                    region.chunks(es).map(|chunk| {
                        let mut buf = [0u8; core::mem::size_of::<gpt::GptEntry>()];
                        let len = buf.len();
                        buf.copy_from_slice(&chunk[..len]);
                        buf
                    }),
                    &hdr,
                );
                std::hint::black_box(crc)
            });
        });
    }
    group.finish();
}

fn bench_read_stream_vs_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpt_read_stream_vs_alloc");

    for &n in &[128usize, 1024, 4096] {
        let sector_size = 512u64;
        let total = 400_000u64;
        // Disque mémoire
        let mut buf = vec![0u8; (sector_size * total) as usize];
        let mut io = MemRimIO::new(&mut buf);

        // MBR protectif
        mbr::write_mbr_protective(&mut io, total).unwrap();

        // Header + entries
        let (hdr, entries) = make_header_and_entries(sector_size, total, n);
        gpt::write_gpt_with_header(&mut io, hdr, &entries, sector_size).unwrap();
        // StreamReader (no-alloc)
        group.bench_with_input(BenchmarkId::new("stream_iter", n), &n, |b, &_n| {
            b.iter(|| {
                let mut reader = GptStreamReader::<_, 4096>::new(&mut io, sector_size).unwrap();
                let mut count = 0usize;
                for e in reader.iter() {
                    let e = e.unwrap();
                    count += (e.end_lba - e.start_lba + 1) as usize;
                }
                std::hint::black_box(count)
            });
        });

        // read_gpt_entries (alloc + parse Vec)
        group.bench_with_input(BenchmarkId::new("alloc_read_entries", n), &n, |b, &_n| {
            b.iter(|| {
                let hdr = gpt::read_gpt_header(&mut io, sector_size).unwrap();
                let entries = gpt::read_gpt_entries(&mut io, &hdr, sector_size).unwrap();
                std::hint::black_box(entries.len())
            });
        });
    }

    group.finish();
}

fn bench_write_stream_vs_alloc(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpt_write_stream_vs_alloc");
    for &n in &[128usize, 1024, 4096] {
        let sector = 512u64;
        let total = 400_000u64;

        group.bench_with_input(BenchmarkId::new("stream_writer", n), &n, |b, &_n| {
            b.iter(|| {
                // Disque vierge et MBR protectif
                let mut buf = vec![0u8; (sector * total) as usize];
                let mut io = MemRimIO::new(&mut buf);
                mbr::write_mbr_protective(&mut io, total).unwrap();

                // Header dimensionné + parts
                let (mut hdr, entries) = make_header_and_entries(sector, total, n);
                hdr.num_entries = entries.len() as u32;
                hdr.entry_size = core::mem::size_of::<gpt::GptEntry>() as u32;

                // Ecriture stream
                let mut w = GptStreamWriter::<_, 4096>::from_header(&mut io, sector, hdr).unwrap();
                w.write_entries(entries.len(), entries.clone().into_iter())
                    .unwrap();
                w.finalize().unwrap();

                std::hint::black_box(entries.len())
            });
        });

        group.bench_with_input(BenchmarkId::new("alloc_writer", n), &n, |b, &_n| {
            b.iter(|| {
                let mut buf = vec![0u8; (sector * total) as usize];
                let mut io = MemRimIO::new(&mut buf);
                mbr::write_mbr_protective(&mut io, total).unwrap();

                let (mut hdr, entries) = make_header_and_entries(sector, total, n);
                hdr.num_entries = entries.len() as u32;
                hdr.entry_size = core::mem::size_of::<gpt::GptEntry>() as u32;

                gpt::write_gpt_with_header(&mut io, hdr, &entries, sector).unwrap();

                std::hint::black_box(entries.len())
            });
        });
    }
    group.finish();
}
