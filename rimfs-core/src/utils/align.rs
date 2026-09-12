// SPDX-License-Identifier: MIT

//! Sector, cluster, and boundary alignment arithmetic helpers.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

/// Align `value` up to the next multiple of `alignment`.
/// `alignment` must be a power of two.
#[inline]
pub fn align_up(value: usize, alignment: usize) -> usize {
    debug_assert!(
        alignment > 0 && (alignment & (alignment - 1)) == 0,
        "Alignment must be power of two"
    );
    (value + alignment - 1) & !(alignment - 1)
}

/// Align `value` down to the previous multiple of `alignment`.
/// `alignment` must be a power of two.
#[inline]
pub fn align_down(value: usize, alignment: usize) -> usize {
    debug_assert!(
        alignment > 0 && (alignment & (alignment - 1)) == 0,
        "Alignment must be power of two"
    );
    value & !(alignment - 1)
}

/// Check if `value` is aligned to `alignment`.
#[inline]
pub fn is_aligned(value: usize, alignment: usize) -> bool {
    (value & (alignment - 1)) == 0
}

/// Pad a vector with zeroes until its length is a multiple of `alignment`.
pub fn pad_vec(vec: &mut Vec<u8>, alignment: usize) {
    let len = vec.len();
    let new_len = align_up(len, alignment);
    if new_len > len {
        vec.resize(new_len, 0);
    }
}

/// Pad a vector with zeroes to reach an exact target size.
pub fn pad_to_size(vec: &mut Vec<u8>, target_size: usize) {
    if vec.len() < target_size {
        vec.resize(target_size, 0);
    }
}
