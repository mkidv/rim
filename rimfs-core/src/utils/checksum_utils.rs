// SPDX-License-Identifier: MIT

//! Fast checksum and digest accumulation helpers.

/// Minimal trait to abstract the rolling "rotate-right then add byte" checksum
/// over different word sizes (u8, u32). This keeps the loop monomorphized and
/// no_std-friendly.
pub trait RollingWord: Copy {
    fn ror1(self) -> Self;
    fn add_byte(self, b: u8) -> Self;
}

impl RollingWord for u8 {
    #[inline(always)]
    fn ror1(self) -> Self {
        self.rotate_right(1)
    }
    #[inline(always)]
    fn add_byte(self, b: u8) -> Self {
        self.wrapping_add(b)
    }
}

impl RollingWord for u32 {
    #[inline(always)]
    fn ror1(self) -> Self {
        self.rotate_right(1)
    }
    #[inline(always)]
    fn add_byte(self, b: u8) -> Self {
        self.wrapping_add(b as u32)
    }
}

impl RollingWord for u16 {
    #[inline(always)]
    fn ror1(self) -> Self {
        self.rotate_right(1)
    }
    #[inline(always)]
    fn add_byte(self, b: u8) -> Self {
        self.wrapping_add(b as u16)
    }
}

/// Core accumulator with an optional escape predicate on (absolute) byte index.
/// The predicate returning true means "skip this byte".
#[inline(always)]
pub fn accumulate_checksum_with_escape<T, F>(sum: &mut T, data: &[u8], mut escape: F)
where
    T: RollingWord,
    F: FnMut(usize, u8) -> bool,
{
    for (i, &b) in data.iter().enumerate() {
        if escape(i, b) {
            continue;
        }
        *sum = sum.ror1().add_byte(b);
    }
}

/// Convenience: accumulate with no escaping.
#[inline(always)]
pub fn accumulate_checksum<T: RollingWord>(sum: &mut T, data: &[u8]) {
    accumulate_checksum_with_escape(sum, data, |_i, _b| false);
}

/// One-shot checksum helpers (no escape).
#[inline(always)]
pub fn checksum<T: RollingWord + Default + Copy>(data: &[u8]) -> T {
    let mut s: T = Default::default(); // works for u8/u32 (both Default = 0)
    accumulate_checksum(&mut s, data);
    s
}

/// Specialization aliases for clarity.
#[inline(always)]
pub fn accumulate_u8(sum: &mut u8, data: &[u8]) {
    accumulate_checksum(sum, data)
}
#[inline(always)]
pub fn accumulate_u16(sum: &mut u16, data: &[u8]) {
    accumulate_checksum(sum, data)
}
#[inline(always)]
pub fn accumulate_u32(sum: &mut u32, data: &[u8]) {
    accumulate_checksum(sum, data)
}
#[inline(always)]
pub fn checksum_u8(data: &[u8]) -> u8 {
    checksum::<u8>(data)
}
#[inline(always)]
pub fn checksum_u16(data: &[u8]) -> u16 {
    checksum::<u16>(data)
}
#[inline(always)]
pub fn checksum_u32(data: &[u8]) -> u32 {
    checksum::<u32>(data)
}

pub use crc32fast::Hasher as Crc32Hasher;

/// CRC-32 (ISO 3309) - Poly: 0xEDB88320
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

/// Compute CRC-32 by streaming in chunks from a reader.
pub fn crc32_reader<R: rimio::RimRead + ?Sized>(
    reader: &mut R,
    offset: u64,
    len: u64,
) -> rimio::RimIOResult<u32> {
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    let mut remaining = len;
    let mut current_offset = offset;

    while remaining > 0 {
        let to_read = remaining.min(buf.len() as u64) as usize;
        reader.read_at(current_offset, &mut buf[..to_read])?;
        hasher.update(&buf[..to_read]);
        current_offset += to_read as u64;
        remaining -= to_read as u64;
    }

    Ok(hasher.finalize())
}

/// CRC-32C (Castagnoli) - Poly: 0x82F63B78
/// Used by Ext4 (metadata checksums) and others.
///
/// NOTE: We implement this manually to avoid adding a heavy dependency on `crc32c`/`crc` crates
/// in `no_std` environments where binary size or dependency tree depth matters.
/// This implementation is optimized for code size rather than throughput.
pub fn crc32c(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0x82F63B78;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// CRC-32C (Castagnoli) with initial seed.
/// Useful for chained checksums (e.g. extending a previous checksum).
pub fn crc32c_seeded(seed: u32, data: &[u8]) -> u32 {
    let mut crc = !seed;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0x82F63B78;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// CRC-16-CCITT (Poly: 0x1021) - Update existing CRC with new data
pub fn crc16_ccitt_update(mut crc: u16, data: &[u8]) -> u16 {
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if (crc & 0x8000) != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// CRC-16-CCITT (Poly: 0x1021, Init: 0xFFFF)
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    crc16_ccitt_update(0xFFFF, data)
}
