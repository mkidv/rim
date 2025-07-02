// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

#[cfg(not(feature = "alloc"))]
use crate::BLOCK_BUF_SIZE;
use crate::prelude::*;

#[cfg(feature = "alloc")]
pub fn compare_streamed_bytes<IO1, IO2>(
    io1: &mut IO1,
    offset1: u64,
    io2: &mut IO2,
    offset2: u64,
    total_bytes: usize,
    chunk_size: usize,
) -> BlockIOResult<bool>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;

    while remaining > 0 {
        let to_read = remaining.min(chunk_size);

        io1.read_at(pos1, &mut buf1[..to_read])?;
        io2.read_at(pos2, &mut buf2[..to_read])?;

        if buf1[..to_read] != buf2[..to_read] {
            return Ok(false);
        }

        pos1 += to_read as u64;
        pos2 += to_read as u64;
        remaining -= to_read;
    }

    Ok(true)
}

#[cfg(not(feature = "alloc"))]
pub fn compare_streamed_bytes<IO1, IO2>(
    io1: &mut IO1,
    offset1: u64,
    io2: &mut IO2,
    offset2: u64,
    total_bytes: usize,
    chunk_size: usize,
) -> BlockIOResult<bool>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    let mut buf1 = [0u8; BLOCK_BUF_SIZE];
    let mut buf2 = [0u8; BLOCK_BUF_SIZE];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;

    while remaining > 0 {
        let chunk = remaining.min(chunk_size).min(BLOCK_BUF_SIZE);

        io1.read_at(pos1, &mut buf1[..chunk])?;
        io2.read_at(pos2, &mut buf2[..chunk])?;

        if &buf1[..chunk] != &buf2[..chunk] {
            return Ok(false);
        }

        pos1 += chunk as u64;
        pos2 += chunk as u64;
        remaining -= chunk;
    }

    Ok(true)
}

#[cfg(feature = "alloc")]
pub fn first_diff_bytes<IO1, IO2>(
    io1: &mut IO1,
    offset1: u64,
    io2: &mut IO2,
    offset2: u64,
    total_bytes: usize,
    chunk_size: usize,
) -> BlockIOResult<Option<(u64, u8, u8)>>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0;

    while remaining > 0 {
        let to_read = remaining.min(chunk_size);

        io1.read_at(pos1, &mut buf1[..to_read])?;
        io2.read_at(pos2, &mut buf2[..to_read])?;

        for i in 0..to_read {
            if buf1[i] != buf2[i] {
                return Ok(Some((global_offset + i as u64, buf1[i], buf2[i])));
            }
        }

        pos1 += to_read as u64;
        pos2 += to_read as u64;
        global_offset += to_read as u64;
        remaining -= to_read;
    }

    Ok(None)
}

#[cfg(not(feature = "alloc"))]
pub fn first_diff_bytes<IO1, IO2>(
    io1: &mut IO1,
    offset1: u64,
    io2: &mut IO2,
    offset2: u64,
    total_bytes: usize,
    chunk_size: usize,
) -> BlockIOResult<Option<(u64, u8, u8)>>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    let mut buf1 = [0u8; BLOCK_BUF_SIZE];
    let mut buf2 = [0u8; BLOCK_BUF_SIZE];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0;

    while remaining > 0 {
        let chunk = remaining.min(chunk_size).min(BLOCK_BUF_SIZE);

        io1.read_at(pos1, &mut buf1[..chunk])?;
        io2.read_at(pos2, &mut buf2[..chunk])?;

        for i in 0..chunk {
            if buf1[i] != buf2[i] {
                return Ok(Some((global_offset + i as u64, buf1[i], buf2[i])));
            }
        }

        pos1 += chunk as u64;
        pos2 += chunk as u64;
        global_offset += chunk as u64;
        remaining -= chunk;
    }

    Ok(None)
}

#[cfg(feature = "std")]
pub fn diff_streamed_bytes_pretty<IO1, IO2>(
    io1: &mut IO1,
    offset1: u64,
    io2: &mut IO2,
    offset2: u64,
    total_bytes: usize,
    chunk_size: usize,
    label: &str,
    max_lines: usize,
    show_context: bool,
) -> BlockIOResult<()>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    use core::cmp::min;
    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0;
    let mut differences = 0;
    let mut context_lines = 0;

    println!("\u{1F50D} Diff [{label}] for {total_bytes} bytes (dual view)");

    while remaining > 0 {
        let to_read = min(remaining, chunk_size);
        io1.read_at(pos1, &mut buf1[..to_read])?;
        io2.read_at(pos2, &mut buf2[..to_read])?;

        for i in (0..to_read).step_by(16) {
            let line_len = min(16, to_read - i);
            let a = &buf1[i..i + line_len];
            let b = &buf2[i..i + line_len];

            let base = global_offset + i as u64;

            if a != b {
                print!("{base:08X}: ");
                for j in 0..line_len {
                    if a[j] == b[j] {
                        print!("{:02X} ", a[j]);
                    } else {
                        print!("\x1b[31m{:02X}\x1b[0m ", a[j]);
                    }
                }

                print!("| ");
                for j in 0..line_len {
                    let ascii = |b: u8| if b.is_ascii_graphic() { b as char } else { '.' };
                    if a[j] == b[j] {
                        print!("{}", ascii(a[j]));
                    } else {
                        print!("\x1b[31m{}\x1b[0m", ascii(a[j]));
                    }
                }

                print!("  =>  ");
                for j in 0..line_len {
                    if a[j] == b[j] {
                        print!("{:02X} ", b[j]);
                    } else {
                        print!("\x1b[32m{:02X}\x1b[0m ", b[j]);
                    }
                }

                print!("| ");
                for j in 0..line_len {
                    let ascii = |b: u8| if b.is_ascii_graphic() { b as char } else { '.' };
                    if a[j] == b[j] {
                        print!("{}", ascii(b[j]));
                    } else {
                        print!("\x1b[32m{}\x1b[0m", ascii(b[j]));
                    }
                }

                println!();
                context_lines = 0;
                differences += 1;
                if differences >= max_lines {
                    println!("... (more differences hidden)");
                    return Ok(());
                }
            } else if show_context && context_lines < max_lines.div_ceil(2) {
                print!("{base:08X}: ");
                for byte in a.iter().take(line_len) {
                    print!("{byte:02X} ");
                }
                print!("| ");
                for byte in a.iter().take(line_len) {
                    let ascii = |b: u8| if b.is_ascii_graphic() { b as char } else { '.' };
                    print!("{}", ascii(*byte));
                }
                          println!();
                context_lines += 1;
                if context_lines >= max_lines.div_ceil(2) {
                    println!("... (identical lines omitted)");
                }
            } 
        }

        pos1 += to_read as u64;
        pos2 += to_read as u64;
        global_offset += to_read as u64;
        remaining -= to_read;
    }

    if differences == 0 {
        println!("\u{2705} No differences found [{label}]");
    }

    Ok(())
}

#[cfg(all(not(feature = "std"), feature = "alloc"))]
pub fn diff_streamed_bytes_pretty<IO1, IO2>(
    _io1: &mut IO1,
    _offset1: u64,
    _io2: &mut IO2,
    _offset2: u64,
    _total_bytes: usize,
    _chunk_size: usize,
    _label: &str,
    _max_lines: usize,
) -> BlockIOResult<()>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    Err(BlockIOError::Error(
        "diff_streamed_bytes_pretty requires std",
    ))
}

#[cfg(feature = "alloc")]
pub fn diff_streamed_bytes_log<IO1, IO2>(
    io1: &mut IO1,
    offset1: u64,
    io2: &mut IO2,
    offset2: u64,
    total_bytes: usize,
    chunk_size: usize,
    max_lines: usize,
) -> BlockIOResult<Vec<(u64, u8, u8)>>
where
    IO1: BlockIO + ?Sized,
    IO2: BlockIO + ?Sized,
{
    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0;

    let mut diffs = Vec::new();

    while remaining > 0 && diffs.len() < max_lines {
        let to_read = remaining.min(chunk_size);

        io1.read_at(pos1, &mut buf1[..to_read])?;
        io2.read_at(pos2, &mut buf2[..to_read])?;

        for i in 0..to_read {
            if buf1[i] != buf2[i] {
                diffs.push((global_offset + i as u64, buf1[i], buf2[i]));
                if diffs.len() >= max_lines {
                    break;
                }
            }
        }

        pos1 += to_read as u64;
        pos2 += to_read as u64;
        global_offset += to_read as u64;
        remaining -= to_read;
    }

    Ok(diffs)
}
