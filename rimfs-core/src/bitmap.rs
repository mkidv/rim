// SPDX-License-Identifier: MIT

//! Generic bit allocation bitmap supporting fast range searches.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use rimio::prelude::*;

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

    /// Sets or clears a range of bits [start, end).
    ///
    /// Bits outside the slice bounds are ignored.
    fn set_bits_in_range(&mut self, start: usize, end: usize, value: bool);

    /// Counts the number of set bits in the given range `[start, end)`.
    fn count_ones_in_range(&self, start: usize, end: usize) -> usize;

    /// Finds the first zero bit starting from `start`.
    ///
    /// Returns `None` if no zero bit is found within the bitmap.
    fn find_first_zero(&self, start: usize) -> Option<usize>;

    /// Counts the total number of set bits in the entire bitmap.
    fn count_ones(&self) -> usize;

    /// Finds a contiguous range of `len` zero bits starting from `start`.
    ///
    /// Returns the start index of the found range, or `None`.
    fn find_next_zero_range(&self, start: usize, len: usize) -> Option<usize>;
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

    fn set_bits_in_range(&mut self, start: usize, end: usize, value: bool) {
        if start >= end {
            return;
        }

        let first_byte = start / 8;
        let last_byte = (end - 1) / 8;

        if first_byte == last_byte {
            // All bits in the same byte
            if let Some(byte) = self.get_mut(first_byte) {
                let mask = (0xFFu8 << (start % 8)) & (0xFFu8 >> (7 - (end - 1) % 8));
                if value {
                    *byte |= mask;
                } else {
                    *byte &= !mask;
                }
            }
            return;
        }

        // Handle first partial byte
        if let Some(byte) = self.get_mut(first_byte) {
            let mask = 0xFFu8 << (start % 8);
            if value {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }

        // Handle full bytes in between
        if last_byte > first_byte + 1 {
            let fill_val = if value { 0xFF } else { 0x00 };
            let range_start = first_byte + 1;
            let range_end = last_byte; // Exclusive
            if range_start < self.len() {
                let actual_end = range_end.min(self.len());
                self[range_start..actual_end].fill(fill_val);
            }
        }

        // Handle last partial byte
        if let Some(byte) = self.get_mut(last_byte) {
            let mask = 0xFFu8 >> (7 - (end - 1) % 8);
            if value {
                *byte |= mask;
            } else {
                *byte &= !mask;
            }
        }
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

    fn find_next_zero_range(&self, start: usize, len: usize) -> Option<usize> {
        if len == 0 {
            return Some(start);
        }

        let mut current_start = start;
        let limit = self.len() * 8;

        while current_start + len <= limit {
            // fast-forward to next zero
            current_start = self.find_first_zero(current_start)?;

            if current_start + len > limit {
                return None;
            }

            let mut all_zero = true;
            for i in 0..len {
                if self.get_bit(current_start + i) {
                    all_zero = false;
                    current_start += i + 1; // Skip past this 1
                    break;
                }
            }

            if all_zero {
                return Some(current_start);
            }
        }
        None
    }
}

#[cfg(any(feature = "std", feature = "alloc"))]
impl BitmapOps for Vec<u8> {
    #[inline]
    fn set_bit(&mut self, bit: usize, value: bool) {
        BitmapOps::set_bit(self.as_mut_slice(), bit, value)
    }

    #[inline]
    fn get_bit(&self, bit: usize) -> bool {
        BitmapOps::get_bit(self.as_slice(), bit)
    }

    #[inline]
    fn set_bits_in_range(&mut self, start: usize, end: usize, value: bool) {
        BitmapOps::set_bits_in_range(self.as_mut_slice(), start, end, value)
    }

    #[inline]
    fn count_ones_in_range(&self, start: usize, end: usize) -> usize {
        BitmapOps::count_ones_in_range(self.as_slice(), start, end)
    }

    #[inline]
    fn find_first_zero(&self, start: usize) -> Option<usize> {
        BitmapOps::find_first_zero(self.as_slice(), start)
    }

    #[inline]
    fn count_ones(&self) -> usize {
        BitmapOps::count_ones(self.as_slice())
    }

    #[inline]
    fn find_next_zero_range(&self, start: usize, len: usize) -> Option<usize> {
        BitmapOps::find_next_zero_range(self.as_slice(), start, len)
    }
}

/// Trait to abstract bitmap location and size.
///
/// This allows `BitmapDriver` to work generic over any filesystem metadata that describes a bitmap
/// (e.g., ExFAT global bitmap, Ext4 block group bitmap).
pub trait BitmapFsMeta {
    /// Absolute offset of the bitmap in bytes.
    fn bitmap_offset(&self) -> u64;

    /// Total size of the bitmap in bytes.
    fn bitmap_size(&self) -> u64;

    /// Number of meaningful allocation bits.
    ///
    /// Defaults to the whole bitmap.
    #[inline]
    fn bitmap_valid_bits(&self) -> u64 {
        self.bitmap_size() * 8
    }
}

impl<M: BitmapFsMeta> BitmapFsMeta for &M {
    #[inline]
    fn bitmap_offset(&self) -> u64 {
        (**self).bitmap_offset()
    }

    #[inline]
    fn bitmap_size(&self) -> u64 {
        (**self).bitmap_size()
    }

    #[inline]
    fn bitmap_valid_bits(&self) -> u64 {
        (**self).bitmap_valid_bits()
    }
}

/// Standalone descriptor implementing [`BitmapFsMeta`] for arbitrary bitmap regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleBitmapMeta {
    pub offset: u64,
    pub size: u64,
    pub valid_bits: u64,
}

impl SimpleBitmapMeta {
    #[inline]
    pub fn new(offset: u64, size: u64, valid_bits: u64) -> Self {
        Self {
            offset,
            size,
            valid_bits,
        }
    }
}

impl BitmapFsMeta for SimpleBitmapMeta {
    #[inline]
    fn bitmap_offset(&self) -> u64 {
        self.offset
    }

    #[inline]
    fn bitmap_size(&self) -> u64 {
        self.size
    }

    #[inline]
    fn bitmap_valid_bits(&self) -> u64 {
        self.valid_bits
    }
}

/// A buffered view into an on-disk bitmap.
///
/// Handles caching a window of the bitmap to reduce I/O.
pub struct BitmapDriver<M: BitmapFsMeta> {
    pub meta: M,
    pub buffer: [u8; 4096], // Fixed 4KB window (common sector/block/cluster size)
    pub window_start: u64,  // Byte offset where the current window starts
    pub valid_len: usize,   // How many bytes in buffer are valid
    pub valid: bool,        // Is the buffer loaded?
    pub dirty: bool,        // Does the buffer need flushing?
}

impl<M: BitmapFsMeta> BitmapDriver<M> {
    pub fn new(meta: M) -> Self {
        Self {
            meta,
            buffer: [0u8; 4096],
            window_start: 0,
            valid_len: 0,
            valid: false,
            dirty: false,
        }
    }

    /// Flush the current window if dirty.
    pub fn flush<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> RimIOResult {
        if self.valid && self.dirty {
            let offset = self.meta.bitmap_offset() + self.window_start;
            io.write_at(offset, &self.buffer[..self.valid_len])?;
            self.dirty = false;
        }
        Ok(())
    }

    fn ensure_loaded_ro<R: RimRead + ?Sized>(
        &mut self,
        io: &mut R,
        byte_offset: u64,
    ) -> RimIOResult {
        if self.valid
            && byte_offset >= self.window_start
            && byte_offset < self.window_start + self.valid_len as u64
        {
            return Ok(());
        }

        if self.valid && self.dirty {
            return Err(RimIOError::Invalid("State"));
        }

        let bitmap_size = self.meta.bitmap_size();
        if byte_offset >= bitmap_size {
            return Err(RimIOError::OutOfBounds);
        }

        let buf_len = self.buffer.len() as u64;
        let new_start = (byte_offset / buf_len) * buf_len;

        let to_read = core::cmp::min(buf_len, bitmap_size - new_start) as usize;

        io.read_at(
            self.meta.bitmap_offset() + new_start,
            &mut self.buffer[..to_read],
        )?;

        self.window_start = new_start;
        self.valid_len = to_read;
        self.valid = true;

        Ok(())
    }

    pub fn get_bit_ro<R: RimRead + ?Sized>(
        &mut self,
        io: &mut R,
        bit_index: u64,
    ) -> RimIOResult<bool> {
        let byte_offset = bit_index / 8;
        self.ensure_loaded_ro(io, byte_offset)?;

        let local_byte = (byte_offset - self.window_start) as usize;

        Ok(self
            .buffer
            .get_bit(local_byte * 8 + (bit_index % 8) as usize))
    }

    /// Ensure the window covers the given byte offset.
    ///
    /// The window is aligned to the buffer size if possible to maximize sequential access.
    fn ensure_loaded<IO: RimIO + ?Sized>(&mut self, io: &mut IO, byte_offset: u64) -> RimIOResult {
        // If current window covers this byte, we are good.
        if self.valid
            && byte_offset >= self.window_start
            && byte_offset < self.window_start + self.valid_len as u64
        {
            return Ok(());
        }

        self.flush(io)?;
        self.ensure_loaded_ro(io, byte_offset)
    }

    /// Set a bit at a specific index.
    pub fn set_bit<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        bit_index: u64,
        value: bool,
    ) -> RimIOResult {
        let byte_offset = bit_index / 8;
        self.ensure_loaded(io, byte_offset)?;

        let local_byte = (byte_offset - self.window_start) as usize;
        self.buffer
            .set_bit(local_byte * 8 + (bit_index % 8) as usize, value);
        self.dirty = true;
        Ok(())
    }

    /// Get the value of a bit at a specific index.
    pub fn get_bit<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        bit_index: u64,
    ) -> RimIOResult<bool> {
        let byte_offset = bit_index / 8;
        if self.valid
            && byte_offset >= self.window_start
            && byte_offset < self.window_start + self.valid_len as u64
        {
            let local_byte = (byte_offset - self.window_start) as usize;
            return Ok(self
                .buffer
                .get_bit(local_byte * 8 + (bit_index % 8) as usize));
        }

        if self.valid && self.dirty {
            self.flush(io)?;
        }
        self.get_bit_ro(io, bit_index)
    }

    /// Set a range of bits.
    ///
    /// Handles window switching if the range crosses window boundaries.
    pub fn set_bits_range<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        start_bit: u64,
        count: u64,
        value: bool,
    ) -> RimIOResult {
        if count == 0 {
            return Ok(());
        }

        let end_bit = start_bit + count;
        let mut executed = 0;

        while executed < count {
            let current_bit = start_bit + executed;
            let byte_offset = current_bit / 8;
            self.ensure_loaded(io, byte_offset)?;

            let window_end_byte = self.window_start + self.valid_len as u64;
            let window_end_bit = window_end_byte * 8;

            let check_limit = window_end_bit.min(end_bit);
            let bits_in_this_window = check_limit - current_bit;

            let local_start_bit = (current_bit % 8) + (byte_offset - self.window_start) * 8;
            let local_end_bit = local_start_bit + bits_in_this_window;

            self.buffer
                .set_bits_in_range(local_start_bit as usize, local_end_bit as usize, value);
            self.dirty = true;

            executed += bits_in_this_window;
        }
        Ok(())
    }

    /// Set a range of bits from a RunList and flush to disk.
    pub fn set_run_list<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        runs: &RunList,
        value: bool,
    ) -> RimIOResult {
        for run in runs.iter() {
            self.set_bits_range(io, run.start, run.length, value)?;
        }
        self.flush(io)?;

        Ok(())
    }

    /// Scan for the next contiguous range of `count` zero bits, starting from `hint_bit`.
    ///
    /// Returns the absolute bit index of the start of the range.
    /// Supports runs that cross 4KB window boundaries and allocations exceeding 32,768 bits.
    pub fn find_next_free_ro<R: RimRead + ?Sized>(
        &mut self,
        io: &mut R,
        hint_bit: u64,
        count: u64,
    ) -> RimIOResult<Option<u64>> {
        let total_bits = self.meta.bitmap_valid_bits();

        if count == 0 {
            return Ok((hint_bit <= total_bits).then_some(hint_bit));
        }

        if hint_bit
            .checked_add(count)
            .is_none_or(|end| end > total_bits)
        {
            return Ok(None);
        }

        let mut run_start: Option<u64> = None;
        let mut run_len = 0u64;
        let mut current_bit = hint_bit;

        while current_bit < total_bits {
            let byte_offset = current_bit / 8;
            self.ensure_loaded_ro(io, byte_offset)?;

            let window_start_bit = self.window_start * 8;
            let window_bits = self.valid_len as u64 * 8;
            let window_end_bit = (window_start_bit + window_bits).min(total_bits);

            while current_bit < window_end_bit {
                if run_start.is_none() {
                    let local_start = (current_bit - window_start_bit) as usize;

                    match self.buffer[..self.valid_len].find_first_zero(local_start) {
                        Some(local_zero) => {
                            let zero_bit = window_start_bit + local_zero as u64;

                            if zero_bit >= window_end_bit {
                                current_bit = window_end_bit;
                                break;
                            }

                            run_start = Some(zero_bit);
                            run_len = 0;
                            current_bit = zero_bit;
                        }

                        None => {
                            current_bit = window_end_bit;
                            break;
                        }
                    }
                }

                if !current_bit.is_multiple_of(8) {
                    let local_bit = (current_bit - window_start_bit) as usize;

                    if self.buffer[..self.valid_len].get_bit(local_bit) {
                        run_start = None;
                        run_len = 0;
                        current_bit += 1;
                    } else {
                        run_len += 1;

                        if run_len == count {
                            return Ok(run_start);
                        }

                        current_bit += 1;
                    }
                } else if current_bit.is_multiple_of(64) && current_bit + 64 <= window_end_bit {
                    let local_byte = ((current_bit - window_start_bit) / 8) as usize;
                    let word = u64::from_le_bytes(
                        self.buffer[local_byte..local_byte + 8].try_into().unwrap(),
                    );
                    if word == 0 {
                        let needed = count - run_len;
                        if needed <= 64 {
                            return Ok(run_start);
                        }
                        run_len += 64;
                        current_bit += 64;
                    } else {
                        let zeros = word.trailing_zeros() as u64;
                        run_len += zeros;
                        if run_len >= count {
                            return Ok(run_start);
                        }
                        current_bit += zeros + 1;
                        run_start = None;
                        run_len = 0;
                    }
                } else {
                    let local_byte = ((current_bit - window_start_bit) / 8) as usize;

                    if current_bit + 8 <= window_end_bit {
                        let byte = self.buffer[local_byte];

                        if byte == 0 {
                            let needed = count - run_len;

                            if needed <= 8 {
                                return Ok(run_start);
                            }

                            run_len += 8;
                            current_bit += 8;
                        } else {
                            let zeros = byte.trailing_zeros() as u64;

                            run_len += zeros;

                            if run_len >= count {
                                return Ok(run_start);
                            }

                            current_bit += zeros + 1;
                            run_start = None;
                            run_len = 0;
                        }
                    } else {
                        let local_bit = (current_bit - window_start_bit) as usize;

                        if self.buffer[..self.valid_len].get_bit(local_bit) {
                            run_start = None;
                            run_len = 0;
                        } else {
                            run_len += 1;

                            if run_len == count {
                                return Ok(run_start);
                            }
                        }

                        current_bit += 1;
                    }
                }
            }
        }

        Ok(None)
    }

    /// Scan for the next contiguous range of `count` zero bits, starting from `hint_bit`.
    ///
    /// Returns the absolute bit index of the start of the range.
    /// Supports runs that cross 4KB window boundaries and allocations exceeding 32,768 bits.
    pub fn find_next_free<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        hint_bit: u64,
        count: u64,
    ) -> RimIOResult<Option<u64>> {
        if self.valid && self.dirty {
            self.flush(io)?;
        }

        self.find_next_free_ro(io, hint_bit, count)
    }

    /// Find enough free units without mutating the bitmap. Ignores padding bits.
    pub fn find_free_runs_ro<R: RimRead + ?Sized>(
        &mut self,
        io: &mut R,
        count: u64,
    ) -> RimIOResult<Option<RunList>> {
        if count == 0 {
            return Err(RimIOError::InvalidBuffer);
        }

        let valid_bits = self.meta.bitmap_valid_bits();

        let mut runs = RunList::new();
        let mut found = 0u64;
        let mut bit = 0u64;

        while bit < valid_bits {
            let Some(start) = self.find_next_free_ro(io, bit, 1)? else {
                break;
            };

            bit = start;

            // Efficiently advance through consecutive free bits directly from windowed memory
            while bit < valid_bits && found < count {
                let byte_offset = bit / 8;
                self.ensure_loaded_ro(io, byte_offset)?;

                let window_start_bit = self.window_start * 8;
                let window_bits = self.valid_len as u64 * 8;
                let window_end_bit = (window_start_bit + window_bits).min(valid_bits);

                let mut in_window = true;
                while bit < window_end_bit && found < count {
                    let local_bit = (bit - window_start_bit) as usize;
                    if bit.is_multiple_of(8) && bit + 8 <= window_end_bit {
                        let byte = self.buffer[local_bit / 8];
                        if byte == 0 {
                            let needed = count - found;
                            let take = needed.min(8);
                            bit += take;
                            found += take;
                            if take < 8 {
                                break;
                            }
                            continue;
                        } else {
                            let zeros = byte.trailing_zeros() as u64;
                            let needed = count - found;
                            let take = zeros.min(needed);
                            bit += take;
                            found += take;
                            in_window = false;
                            break;
                        }
                    } else if !self.buffer[..self.valid_len].get_bit(local_bit) {
                        bit += 1;
                        found += 1;
                    } else {
                        in_window = false;
                        break;
                    }
                }

                if !in_window {
                    break;
                }
            }

            runs.push(Run::new(start, bit - start));

            if found == count {
                return Ok(Some(runs));
            }

            bit += 1;
        }

        Ok(None)
    }

    /// Find enough free units without mutating the bitmap. Ignores padding bits.
    pub fn find_free_runs<IO: RimIO + ?Sized>(
        &mut self,
        io: &mut IO,
        count: u64,
    ) -> RimIOResult<Option<RunList>> {
        if self.valid && self.dirty {
            self.flush(io)?;
        }

        self.find_free_runs_ro(io, count)
    }

    /// Optimized: Format the entire bitmap with a pattern (usually 0x00 or 0xFF).
    /// Used for initialization. Uses zero_at when pattern is 0 for sparse storage optimization.
    pub fn format_with<IO: RimIO + ?Sized>(&mut self, io: &mut IO, pattern: u8) -> RimIOResult {
        let size = self.meta.bitmap_size();
        if pattern == 0 {
            io.zero_at(self.meta.bitmap_offset(), size)?;
        } else {
            let chunk_size = self.buffer.len();
            self.buffer.fill(pattern);

            let mut offset = 0;
            while offset < size {
                let to_write = core::cmp::min(chunk_size as u64, size - offset) as usize;
                let abs_offset = self.meta.bitmap_offset() + offset;
                io.write_at(abs_offset, &self.buffer[..to_write])?;
                offset += to_write as u64;
            }
        }

        // Invalidate current window to be safe
        self.valid = false;
        self.dirty = false;
        Ok(())
    }

    /// Count used bits without requiring write access.
    pub fn count_ones_ro<R: RimRead + ?Sized>(&mut self, io: &mut R) -> RimIOResult<u64> {
        let valid_bits = self.meta.bitmap_valid_bits();

        let full_bytes = valid_bits / 8;
        let tail_bits = (valid_bits % 8) as u32;

        let mut count = 0u64;
        let mut offset = 0u64;

        while offset < full_bytes {
            self.ensure_loaded_ro(io, offset)?;

            let available = self.valid_len as u64;
            let remaining = full_bytes - offset;
            let len = available.min(remaining) as usize;

            count += self.buffer[..len]
                .iter()
                .map(|b| b.count_ones() as u64)
                .sum::<u64>();

            offset += len as u64;
        }

        if tail_bits != 0 {
            self.ensure_loaded_ro(io, full_bytes)?;

            let local_byte = (full_bytes - self.window_start) as usize;
            let mask = (1u8 << tail_bits) - 1;

            count += (self.buffer[local_byte] & mask).count_ones() as u64;
        }

        Ok(count)
    }

    /// Count used bits, flushing pending writes first.
    pub fn count_ones<IO: RimIO + ?Sized>(&mut self, io: &mut IO) -> RimIOResult<u64> {
        if self.valid && self.dirty {
            self.flush(io)?;
        }

        self.count_ones_ro(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_set_get_bit() {
        let mut bitmap = [0u8; 4];

        bitmap.set_bit(0, true);
        assert!(bitmap.get_bit(0));
        assert_eq!(bitmap[0], 0b00000001);

        bitmap.set_bit(7, true);
        assert!(bitmap.get_bit(7));
        assert_eq!(bitmap[0], 0b10000001);

        bitmap.set_bit(8, true);
        assert!(bitmap.get_bit(8));
        assert_eq!(bitmap[1], 0b00000001);

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
        let mut bitmap = [0u8; 3];
        bitmap[0] = 0b11111111;
        bitmap[1] = 0b11111110;

        assert_eq!(bitmap.find_first_zero(0), Some(8));

        let bitmap2 = [0b11111111u8, 0b11111101, 0b00000000];
        assert_eq!(bitmap2.find_first_zero(0), Some(9));

        // All ones
        let full = [0xFFu8; 4];
        assert_eq!(full.find_first_zero(0), None);
    }

    #[test]
    fn test_find_next_zero_range() {
        // 00000000 11111111 00000000
        let bitmap = [0x00, 0xFF, 0x00];

        // Simple finds
        assert_eq!(bitmap.find_next_zero_range(0, 8), Some(0));
        assert_eq!(bitmap.find_next_zero_range(0, 9), None); // Interrupted by 0xFF at bit 8

        assert_eq!(bitmap.find_next_zero_range(0, 10), None);
        // Should hop over 0xFF(bits 8-15) to bit 16
        assert_eq!(bitmap.find_next_zero_range(5, 5), Some(16));
    }

    #[test]
    fn test_set_bits_in_range() {
        let mut bitmap = [0u8; 4];

        // Within one byte: [2, 6) -> bits 2, 3, 4, 5
        bitmap.set_bits_in_range(2, 6, true);
        assert_eq!(bitmap[0], 0b00111100);
        assert_eq!(bitmap[1], 0);

        bitmap.set_bits_in_range(3, 5, false);
        assert_eq!(bitmap[0], 0b00100100);

        // Multiple bytes: [6, 20) -> bits 6..7, 8..15, 16..19
        bitmap = [0u8; 4];
        bitmap.set_bits_in_range(6, 20, true);
        assert_eq!(bitmap[0], 0b11000000);
        assert_eq!(bitmap[1], 0b11111111);
        assert_eq!(bitmap[2], 0b00001111);
        assert_eq!(bitmap[3], 0);

        // Full block set
        bitmap = [0u8; 4];
        bitmap.set_bits_in_range(0, 32, true);
        assert_eq!(bitmap, [0xFFu8; 4]);

        // Out of bounds
        let mut bitmap_small = [0u8; 2];
        bitmap_small.set_bits_in_range(10, 30, true);
        assert_eq!(bitmap_small[0], 0);
        // byte 1 is bits 8..15. [10, 30) intersects with [8, 16) at 10..15
        assert_eq!(bitmap_small[1], 0b11111100u8);
    }

    struct TestBitmapMeta {
        size: u64,
        valid_bits: Option<u64>,
    }
    impl BitmapFsMeta for TestBitmapMeta {
        fn bitmap_offset(&self) -> u64 {
            0
        }
        fn bitmap_size(&self) -> u64 {
            self.size
        }
        fn bitmap_valid_bits(&self) -> u64 {
            self.valid_bits.unwrap_or(self.size * 8)
        }
    }

    #[test]
    fn test_find_next_free_cross_window_and_large() {
        let meta = TestBitmapMeta {
            size: 16384,
            valid_bits: None,
        };
        let mut data = vec![0xFFu8; 16384];
        let mut driver = BitmapDriver::new(&meta);
        let mut io = MemRimIO::new(&mut data);

        assert_eq!(driver.find_next_free(&mut io, 0, 1).unwrap(), None);

        driver.set_bits_range(&mut io, 32700, 100, false).unwrap();
        driver.flush(&mut io).unwrap();

        assert_eq!(driver.find_next_free(&mut io, 0, 100).unwrap(), Some(32700));
        assert_eq!(
            driver.find_next_free(&mut io, 32750, 50).unwrap(),
            Some(32750)
        );
        assert_eq!(driver.find_next_free(&mut io, 32700, 101).unwrap(), None);

        driver.set_bits_range(&mut io, 10000, 40000, false).unwrap();
        driver.flush(&mut io).unwrap();
        assert_eq!(
            driver.find_next_free(&mut io, 0, 40000).unwrap(),
            Some(10000)
        );
    }

    #[test]
    fn fragmented_scan_excludes_padding_and_does_not_reserve() {
        let meta = TestBitmapMeta {
            size: 2,
            valid_bits: Some(8),
        };
        let mut data = [0b10101010, 0b11111110];
        let mut io = rimio::prelude::MemRimIO::new(&mut data);
        let mut driver = BitmapDriver::new(&meta);
        let runs = driver.find_free_runs(&mut io, 4).unwrap().unwrap();
        assert_eq!(runs.total_units(), 4);
        assert_eq!(
            runs.iter()
                .map(|run| run.start)
                .collect::<alloc::vec::Vec<_>>(),
            [0, 2, 4, 6]
        );
        assert!(driver.find_free_runs(&mut io, 5).unwrap().is_none());
        assert_eq!(
            driver
                .find_free_runs(&mut io, 4)
                .unwrap()
                .unwrap()
                .total_units(),
            4
        );
        driver.flush(&mut io).unwrap();
        assert_eq!(data, [0b10101010, 0b11111110]);
    }
}
