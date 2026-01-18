// cargo run -p rimpart --example gpt_roundtrip --features std,mem
use rimio::prelude::MemRimIO;
use rimpart::gpt::{self, GptEntry};
use rimpart::guids;
use rimpart::mbr;
use rimpart::scanner::scan_disk;

fn main() {
    let sector = 512u64;
    let total = 20_000u64; // ~10 MiB
    let mut buf = vec![0u8; (sector * total) as usize];
    let mut io = MemRimIO::new(&mut buf);

    mbr::write_mbr_protective(&mut io, total).expect("mbr write failed");

    let esp = GptEntry::new(guids::GPT_PARTITION_TYPE_ESP, [1; 16], 2048, 4095, 0, "ESP");
    let root = GptEntry::new(
        guids::GPT_PARTITION_TYPE_LINUX,
        [2; 16],
        4096,
        9999,
        0,
        "rootfs",
    );

    gpt::write_gpt_from_entries(&mut io, &[esp, root], total, [0xAB; 16])
        .expect("gpt write failed");

    let info = scan_disk(&mut io).expect("scan failed");
    println!("{info}");
}
