// SPDX-License-Identifier: MIT

//! Bitmap operations trait for efficient bit manipulation.
//!
//! Provides a unified interface for setting, getting, and counting bits
//! in byte slices used as bitmaps (allocation bitmaps, reachability tracking, etc.).

/// Extension trait for bitmap operations on byte slices.
///
/// All operations use little-endian bit ordering within bytes:
/// - Bit 0 is the LSB of byte 0
/// - Bit 7 is the MSB of byte 0
/// - Bit 8 is the LSB of byte 1, etc.
pub trait BitmapOps {
    /// Sets or clears a bit at the given position.
    ///
    /// Does nothing if `bit` is out of bounds.
    fn set_bit(&mut self, bit: usize, value: bool);

    /// Gets the value of a bit at the given position.
    ///
    /// Returns `false` if `bit` is out of bounds.
    fn get_bit(&self, bit: usize) -> bool;

    /// Counts the number of set bits in the given range `[start, end)`.
    fn count_ones_in_range(&self, start: usize, end: usize) -> usize;

    /// Finds the first zero bit starting from `start`.
    ///
    /// Returns `None` if no zero bit is found within the bitmap.
    fn find_first_zero(&self, start: usize) -> Option<usize>;

    /// Counts the total number of set bits in the entire bitmap.
    fn count_ones(&self) -> usize;
}

impl BitmapOps for [u8] {
    #[inline]
    fn set_bit(&mut self, bit: usize, value: bool) {
        if let Some(byte) = self.get_mut(bit / 8) {
            let mask = 1u8 << (bit % 8);
            if value {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }
    }

    #[inline]
    fn get_bit(&self, bit: usize) -> bool {
        self.get(bit / 8)
            .is_some_and(|b| (b & (1 << (bit % 8))) != 0)
    }

    fn count_ones_in_range(&self, start: usize, end: usize) -> usize {
        (start..end).filter(|&i| self.get_bit(i)).count()
    }

    fn find_first_zero(&self, start: usize) -> Option<usize> {
        let total_bits = self.len() * 8;
        // Start from the byte containing `start`
        let start_byte = start / 8;
        let start_bit_in_byte = start % 8;

        for (byte_idx, &byte) in self.iter().enumerate().skip(start_byte) {
            // If the byte is all 1s, skip it
            if byte == 0xFF {
                continue;
            }

            // Check each bit in this byte
            let first_bit = if byte_idx == start_byte {
                start_bit_in_byte
            } else {
                0
            };

            for bit_in_byte in first_bit..8 {
                let bit_idx = byte_idx * 8 + bit_in_byte;
                if bit_idx >= total_bits {
                    return None;
                }
                if (byte & (1 << bit_in_byte)) == 0 {
                    return Some(bit_idx);
                }
            }
        }
        None
    }

    fn count_ones(&self) -> usize {
        self.iter().map(|b| b.count_ones() as usize).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_get_bit() {
        let mut bitmap = [0u8; 4];

        // Set bit 0
        bitmap.set_bit(0, true);
        assert!(bitmap.get_bit(0));
        assert_eq!(bitmap[0], 0b00000001);

        // Set bit 7
        bitmap.set_bit(7, true);
        assert!(bitmap.get_bit(7));
        assert_eq!(bitmap[0], 0b10000001);

        // Set bit 8 (first bit of second byte)
        bitmap.set_bit(8, true);
        assert!(bitmap.get_bit(8));
        assert_eq!(bitmap[1], 0b00000001);

        // Clear bit 0
        bitmap.set_bit(0, false);
        assert!(!bitmap.get_bit(0));
        assert_eq!(bitmap[0], 0b10000000);
    }

    #[test]
    fn test_out_of_bounds() {
        let mut bitmap = [0u8; 2];

        // Out of bounds set should do nothing
        bitmap.set_bit(100, true);
        assert_eq!(bitmap, [0, 0]);

        // Out of bounds get should return false
        assert!(!bitmap.get_bit(100));
    }

    #[test]
    fn test_count_ones() {
        let bitmap = [0b10101010u8, 0b11110000, 0b00001111];

        assert_eq!(bitmap.count_ones(), 4 + 4 + 4);
        assert_eq!(bitmap.count_ones_in_range(0, 8), 4);
        assert_eq!(bitmap.count_ones_in_range(8, 16), 4);
    }

    #[test]
    fn test_find_first_zero() {
        let bitmap = [0b11111111u8, 0b11111110, 0b00000000];

        // First byte is full, first zero is bit 8
        assert_eq!(bitmap.find_first_zero(0), Some(8));

        // Start from bit 9, should find bit 9
        let bitmap2 = [0b11111111u8, 0b11111101, 0b00000000];
        assert_eq!(bitmap2.find_first_zero(0), Some(9));

        // All ones
        let full = [0xFFu8; 4];
        assert_eq!(full.find_first_zero(0), None);
    }
}
