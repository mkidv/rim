// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::String;
use rimfs_core::allocator::FsHandle;

/// TAR block size in bytes.
pub const TAR_BLOCK_SIZE: usize = 512;

/// Standard POSIX ustar magic bytes.
pub const USTAR_MAGIC: &[u8; 6] = b"ustar\0";
pub const USTAR_VERSION: &[u8; 2] = b"00";

/// TAR typeflag constants.
pub const REGTYPE: u8 = b'0';
pub const AREGTYPE: u8 = b'\0';
pub const LNKTYPE: u8 = b'1';
pub const SYMTYPE: u8 = b'2';
pub const CHRTYPE: u8 = b'3';
pub const BLKTYPE: u8 = b'4';
pub const DIRTYPE: u8 = b'5';
pub const FIFOTYPE: u8 = b'6';
pub const GNULONGNAME: u8 = b'L';
pub const GNULONGLINK_TARGET: u8 = b'K';

/// A lightweight handle representing an offset in a TAR archive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TarHandle(pub u64);
impl FsHandle for TarHandle {}
/// A parsed entry from a TAR archive.
///
/// Stream-first: retains offset and size within the source storage.
#[derive(Debug, Clone)]
pub struct TarEntry<'a> {
    pub name: String,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub mtime: u64,
    pub typeflag: u8,
    pub link_name: String,
    /// Offset in the source stream where file payload starts.
    pub data_offset: u64,
    /// Direct borrowed slice when reading from an in-memory buffer.
    pub data_slice: Option<&'a [u8]>,
}

impl<'a> TarEntry<'a> {
    #[inline]
    pub fn is_dir(&self) -> bool {
        self.typeflag == DIRTYPE || self.name.ends_with('/')
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        self.typeflag == REGTYPE || self.typeflag == AREGTYPE
    }

    #[inline]
    pub fn is_symlink(&self) -> bool {
        self.typeflag == SYMTYPE
    }
}

/// Parses an octal ASCII field from a TAR header.
pub fn parse_octal(bytes: &[u8]) -> u64 {
    let mut val: u64 = 0;
    for &b in bytes {
        if b == 0 || b == b' ' {
            break;
        }
        if (b'0'..=b'7').contains(&b) {
            val = (val << 3) | ((b - b'0') as u64);
        }
    }
    val
}

/// Formats a value as an octal ASCII field into a buffer.
pub fn format_octal(buf: &mut [u8], val: u64) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    buf[len - 1] = 0; // null terminator
    let digits_len = len.saturating_sub(1);
    let mut v = val;
    for i in (0..digits_len).rev() {
        buf[i] = b'0' + (v & 7) as u8;
        v >>= 3;
    }
}

/// Calculates the standard TAR checksum (treating checksum bytes as spaces 0x20).
pub fn calculate_checksum(header: &[u8; TAR_BLOCK_SIZE]) -> u32 {
    let mut sum: u32 = 0;
    for (i, &b) in header.iter().enumerate() {
        if (148..156).contains(&i) {
            sum += b' ' as u32;
        } else {
            sum += b as u32;
        }
    }
    sum
}
