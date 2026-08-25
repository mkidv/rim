// SPDX-License-Identifier: MIT
//! NTFS runlist decoding (mapping pairs / data runs)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtfsRun {
    /// Absolute LCN (if sparse, lcn is None)
    pub lcn: Option<u64>,
    /// Length in clusters
    pub len: u64,
}

/// A lightweight runlist view over raw NTFS mapping pairs.
/// No allocation; parsing happens during iteration.
#[derive(Debug, Clone, Copy)]
pub struct NtfsRunList<'a> {
    bytes: &'a [u8],
}

impl<'a> NtfsRunList<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    pub fn iter(self) -> NtfsRunIter<'a> {
        NtfsRunIter {
            input: self.bytes,
            current_lcn: 0i64,
            done: false,
        }
    }
}

pub struct NtfsRunIter<'a> {
    input: &'a [u8],
    current_lcn: i64,
    done: bool,
}

impl<'a> Iterator for NtfsRunIter<'a> {
    type Item = NtfsRun;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let (&header, rest) = self.input.split_first()?;
        self.input = rest;

        if header == 0x00 {
            self.done = true;
            return None;
        }

        let len_size = (header & 0x0F) as usize;
        let off_size = ((header >> 4) & 0x0F) as usize;

        if self.input.len() < len_size + off_size {
            // malformed; stop
            self.done = true;
            return None;
        }

        let (len_bytes, tail) = self.input.split_at(len_size);
        let length = decode_le_u(len_bytes);

        let (off_bytes, tail2) = tail.split_at(off_size);
        let offset = decode_le_i(off_bytes); // signed delta
        self.input = tail2;

        if off_size == 0 {
            // Sparse run: only length; LCN not present and current_lcn must not change.
            return Some(NtfsRun {
                lcn: None,
                len: length,
            });
        }

        self.current_lcn = self.current_lcn.saturating_add(offset);
        Some(NtfsRun {
            lcn: Some(self.current_lcn as u64),
            len: length,
        })
    }
}

fn decode_le_u(bytes: &[u8]) -> u64 {
    let mut v = 0u64;
    for (i, &b) in bytes.iter().enumerate() {
        v |= (b as u64) << (i * 8);
    }
    v
}

fn decode_le_i(bytes: &[u8]) -> i64 {
    if bytes.is_empty() {
        return 0;
    }
    let mut v = 0i64;
    for (i, &b) in bytes.iter().enumerate() {
        v |= (b as i64) << (i * 8);
    }

    // sign-extend if needed
    let last = *bytes.last().unwrap();
    if bytes.len() < 8 && (last & 0x80) != 0 {
        v |= -1i64 << (bytes.len() * 8);
    }
    v
}
