use indicatif::{ProgressBar, ProgressStyle};
use std::io::{Read, Seek, Write};

pub fn copy_with_progress<R: Read, W: Write + Seek>(
    reader: &mut R,
    writer: &mut W,
    total_size: u64,
    message: &str,
) -> anyhow::Result<u64> {
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.white}] {bytes}/{total_bytes} (ETA {eta_precise}) {msg}")
            .unwrap()
            .progress_chars("█░░"),
    );
    pb.set_message(message.to_string());

    let mut buffer = [0u8; 64 * 1024]; // 64KB buffer for better throughput
    let mut copied = 0u64;

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }

        // Check if block is all zeros
        if buffer[..n].iter().all(|&b| b == 0) {
            writer.seek(std::io::SeekFrom::Current(n as i64))?;
        } else {
            writer.write_all(&buffer[..n])?;
        }

        copied += n as u64;
        pb.inc(n as u64);
    }

    pb.finish_and_clear();
    Ok(copied)
}
