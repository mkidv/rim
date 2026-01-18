// SPDX-License-Identifier: MIT

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
pub fn accumulate_u32(sum: &mut u32, data: &[u8]) {
    accumulate_checksum(sum, data)
}
#[inline(always)]
pub fn checksum_u8(data: &[u8]) -> u8 {
    checksum::<u8>(data)
}
#[inline(always)]
pub fn checksum_u32(data: &[u8]) -> u32 {
    checksum::<u32>(data)
}
