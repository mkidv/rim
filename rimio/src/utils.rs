// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

#[cfg(not(feature = "alloc"))]
use crate::BLOCK_BUF_SIZE;
use crate::prelude::*;

/// Common range configuration for streamed comparisons/diffs.
#[derive(Clone, Copy, Debug)]
pub struct DiffRange {
    pub offset1: u64,
    pub offset2: u64,
    pub total_bytes: usize,
    pub chunk_size: usize,
}

impl DiffRange {
    /// Create a range config. Panics if `chunk_size == 0`.
    pub fn new(offset1: u64, offset2: u64, total_bytes: usize, chunk_size: usize) -> Self {
        assert!(chunk_size > 0, "chunk_size must be > 0");
        Self {
            offset1,
            offset2,
            total_bytes,
            chunk_size,
        }
    }
}

impl Default for DiffRange {
    fn default() -> Self {
        // Safe defaults for most uses; 4 KiB chunk is a good baseline.
        Self {
            offset1: 0,
            offset2: 0,
            total_bytes: 0,
            chunk_size: crate::BLOCK_BUF_SIZE,
        }
    }
}

#[cfg(feature = "std")]
#[derive(Clone, Copy, Debug)]
pub struct DiffPrettyOptions<'a> {
    pub label: &'a str,
    pub max_lines: usize,
    pub show_context: bool,
}

#[cfg(feature = "std")]
impl<'a> DiffPrettyOptions<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            max_lines: 32,
            show_context: false,
        }
    }
    pub fn with_max_lines(mut self, n: usize) -> Self {
        self.max_lines = n;
        self
    }
    pub fn with_context(mut self, yes: bool) -> Self {
        self.show_context = yes;
        self
    }
}

#[cfg(feature = "std")]
impl<'a> Default for DiffPrettyOptions<'a> {
    fn default() -> Self {
        Self {
            label: "",
            max_lines: 32,
            show_context: false,
        }
    }
}

#[cfg(feature = "alloc")]
#[derive(Clone, Copy, Debug)]
pub struct DiffLogOptions {
    /// Maximum number of differences to collect.
    pub max_diffs: usize,
}

#[cfg(feature = "alloc")]
impl DiffLogOptions {
    pub fn new(max_diffs: usize) -> Self {
        Self { max_diffs }
    }
}

#[cfg(feature = "alloc")]
impl Default for DiffLogOptions {
    fn default() -> Self {
        Self { max_diffs: 64 }
    }
}

//
// compare_streamed_bytes
//

#[cfg(feature = "alloc")]
pub fn compare_streamed_bytes<IO1, IO2>(
    io1: &mut IO1,
    io2: &mut IO2,
    range: DiffRange,
) -> RimIOResult<bool>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    debug_assert!(range.chunk_size > 0, "chunk_size must be > 0");

    let DiffRange {
        offset1,
        offset2,
        total_bytes,
        chunk_size,
    } = range;

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
    io2: &mut IO2,
    range: DiffRange,
) -> RimIOResult<bool>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    debug_assert!(range.chunk_size > 0, "chunk_size must be > 0");

    let DiffRange {
        offset1,
        offset2,
        total_bytes,
        chunk_size,
    } = range;

    let mut buf1 = [0u8; BLOCK_BUF_SIZE];
    let mut buf2 = [0u8; BLOCK_BUF_SIZE];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;

    while remaining > 0 {
        let chunk = remaining.min(chunk_size).min(BLOCK_BUF_SIZE);

        io1.read_at(pos1, &mut buf1[..chunk])?;
        io2.read_at(pos2, &mut buf2[..chunk])?;

        if buf1[..chunk] != buf2[..chunk] {
            return Ok(false);
        }

        pos1 += chunk as u64;
        pos2 += chunk as u64;
        remaining -= chunk;
    }

    Ok(true)
}

//
// first_diff_bytes
//

#[cfg(feature = "alloc")]
pub fn first_diff_bytes<IO1, IO2>(
    io1: &mut IO1,
    io2: &mut IO2,
    range: DiffRange,
) -> RimIOResult<Option<(u64, u8, u8)>>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    debug_assert!(range.chunk_size > 0, "chunk_size must be > 0");

    let DiffRange {
        offset1,
        offset2,
        total_bytes,
        chunk_size,
    } = range;

    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0u64;

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
    io2: &mut IO2,
    range: DiffRange,
) -> RimIOResult<Option<(u64, u8, u8)>>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    debug_assert!(range.chunk_size > 0, "chunk_size must be > 0");

    let DiffRange {
        offset1,
        offset2,
        total_bytes,
        chunk_size,
    } = range;

    let mut buf1 = [0u8; BLOCK_BUF_SIZE];
    let mut buf2 = [0u8; BLOCK_BUF_SIZE];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0u64;

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

//
// diff_streamed_bytes_pretty (stdout)
//

#[cfg(feature = "std")]
pub fn diff_streamed_bytes_pretty<IO1, IO2>(
    io1: &mut IO1,
    io2: &mut IO2,
    range: DiffRange,
    opts: DiffPrettyOptions<'_>,
) -> RimIOResult<()>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    debug_assert!(range.chunk_size > 0, "chunk_size must be > 0");

    use core::cmp::min;
    let DiffRange {
        offset1,
        offset2,
        total_bytes,
        chunk_size,
    } = range;
    let DiffPrettyOptions {
        label,
        max_lines,
        show_context,
    } = opts;

    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0u64;
    let mut differences = 0usize;
    let mut context_lines = 0usize;

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
                    let ascii = |bb: u8| {
                        if bb.is_ascii_graphic() {
                            bb as char
                        } else {
                            '.'
                        }
                    };
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
                    let ascii = |bb: u8| {
                        if bb.is_ascii_graphic() {
                            bb as char
                        } else {
                            '.'
                        }
                    };
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
                    let ascii = |bb: u8| {
                        if bb.is_ascii_graphic() {
                            bb as char
                        } else {
                            '.'
                        }
                    };
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
    _io2: &mut IO2,
    _range: DiffRange,
    _opts: (),
) -> RimIOResult<()>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    Err(RimIOError::Other("diff_streamed_bytes_pretty requires std"))
}

//
// diff_streamed_bytes_log (collect)
//

#[cfg(feature = "alloc")]
pub fn diff_streamed_bytes_log<IO1, IO2>(
    io1: &mut IO1,
    io2: &mut IO2,
    range: DiffRange,
    opts: DiffLogOptions,
) -> RimIOResult<Vec<(u64, u8, u8)>>
where
    IO1: RimIO + ?Sized,
    IO2: RimIO + ?Sized,
{
    debug_assert!(range.chunk_size > 0, "chunk_size must be > 0");

    let DiffRange {
        offset1,
        offset2,
        total_bytes,
        chunk_size,
    } = range;

    let DiffLogOptions { max_diffs } = opts;

    let mut buf1 = vec![0u8; chunk_size];
    let mut buf2 = vec![0u8; chunk_size];

    let mut remaining = total_bytes;
    let mut pos1 = offset1;
    let mut pos2 = offset2;
    let mut global_offset = 0u64;

    let mut diffs = Vec::new();

    while remaining > 0 && diffs.len() < max_diffs {
        let to_read = remaining.min(chunk_size);

        io1.read_at(pos1, &mut buf1[..to_read])?;
        io2.read_at(pos2, &mut buf2[..to_read])?;

        for i in 0..to_read {
            if buf1[i] != buf2[i] {
                diffs.push((global_offset + i as u64, buf1[i], buf2[i]));
                if diffs.len() >= max_diffs {
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

#[cfg(all(test, feature = "std", feature = "mem"))]
mod tests {
    use super::*;

    fn make_ios(size: usize) -> (MemRimIO<'static>, MemRimIO<'static>) {
        let mut a = vec![0u8; size].into_boxed_slice();
        let mut b = vec![0u8; size].into_boxed_slice();

        for (i, v) in a.iter_mut().enumerate() {
            *v = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
        for (i, v) in b.iter_mut().enumerate() {
            *v = (i as u8).wrapping_mul(3).wrapping_add(1);
        }

        let a: &'static mut [u8] = Box::leak(a);
        let b: &'static mut [u8] = Box::leak(b);

        (MemRimIO::new(a), MemRimIO::new(b))
    }

    #[test]
    fn compare_equal() {
        let (mut io1, mut io2) = make_ios(1024);
        let range = DiffRange {
            offset1: 0,
            offset2: 0,
            total_bytes: 1024,
            chunk_size: 128,
        };
        let eq = compare_streamed_bytes(&mut io1, &mut io2, range).unwrap();
        assert!(eq);
    }

    #[test]
    fn compare_differs() {
        let (mut io1, mut io2) = make_ios(1024);
        io2.write_at(123, &[0xFF, 0xEE, 0xDD, 0xCC]).unwrap();

        let range = DiffRange {
            offset1: 0,
            offset2: 0,
            total_bytes: 1024,
            chunk_size: 64,
        };
        let eq = compare_streamed_bytes(&mut io1, &mut io2, range).unwrap();
        assert!(!eq);
    }

    #[test]
    fn first_diff_found_and_none() {
        let (mut io1, mut io2) = make_ios(256);
        io1.write_at(0, &[0xAA]).unwrap();

        let range = DiffRange {
            offset1: 0,
            offset2: 0,
            total_bytes: 256,
            chunk_size: 32,
        };
        let d = first_diff_bytes(&mut io1, &mut io2, range).unwrap();
        assert_eq!(d, Some((0, 0xAA, 0x01)));

        io2.write_at(0, &[0xAA]).unwrap();
        let d2 = first_diff_bytes(&mut io1, &mut io2, range).unwrap();
        assert_eq!(d2, None);
    }

    #[test]
    fn diff_log_collects_limited_diffs() {
        let (mut io1, mut io2) = make_ios(512);

        io1.write_at(10, &[0x11]).unwrap();
        io2.write_at(10, &[0x22]).unwrap();
        io1.write_at(100, &[0x33]).unwrap();
        io2.write_at(100, &[0x44]).unwrap();
        io1.write_at(255, &[0x55]).unwrap();
        io2.write_at(255, &[0x66]).unwrap();

        let range = DiffRange {
            offset1: 0,
            offset2: 0,
            total_bytes: 512,
            chunk_size: 64,
        };
        let opts = DiffLogOptions { max_diffs: 2 };

        let diffs = diff_streamed_bytes_log(&mut io1, &mut io2, range, opts).unwrap();
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].0, 10);
        assert_eq!(diffs[1].0, 100);
    }

    #[test]
    fn diff_pretty_smoke() {
        let (mut io1, mut io2) = make_ios(128);
        io2.write_at(5, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();

        let range = DiffRange {
            offset1: 0,
            offset2: 0,
            total_bytes: 128,
            chunk_size: 32,
        };
        let opts = DiffPrettyOptions {
            label: "smoke",
            max_lines: 4,
            show_context: true,
        };

        diff_streamed_bytes_pretty(&mut io1, &mut io2, range, opts).unwrap();
    }
}
