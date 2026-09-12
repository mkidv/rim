// SPDX-License-Identifier: MIT
//! NTFS Quota ($Quota) structures and constants
//!
//! Reference: Microsoft NTFS on-disk format & ntfs-3g

use zerocopy::byteorder::little_endian::{I64, U32, U64};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const QUOTA_VERSION_2: u32 = 2;
pub const QUOTA_OWNER_ID_DEFAULT: u32 = 1;
pub const QUOTA_OWNER_ID_ADMINS: u32 = 256;
pub const QUOTA_UNLIMITED: i64 = -1;

bitflags::bitflags! {
    /// NTFS Quota Control Flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    #[repr(transparent)]
    pub struct QuotaFlags: u32 {
        const DEFAULT_LIMITS       = 0x0000_0001;
        const LIMITS_OUT_OF_DATE   = 0x0000_0002;
        const LOG_THRESHOLD        = 0x0000_0004;
        const LOG_LIMIT            = 0x0000_0008;
    }
}

/// Quota user control block ($Quota:$Q data)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct QuotaQData {
    /// Quota record version (always 2 on modern NTFS)
    pub version: U32,
    /// Quota flags (e.g. DEFAULT_LIMITS)
    pub flags: U32,
    /// Number of bytes used by this owner
    pub bytes_used: U64,
    /// Last change timestamp in FILETIME format
    pub change_time: U64,
    /// Soft quota warning threshold (-1 for unlimited)
    pub warning_threshold: I64,
    /// Hard quota limit (-1 for unlimited)
    pub hard_limit: I64,
    /// Peak quota usage reached
    pub peak_quota: U64,
}

impl QuotaQData {
    pub const fn new_unlimited(flags: u32, change_time: u64) -> Self {
        Self {
            version: U32::new(QUOTA_VERSION_2),
            flags: U32::new(flags),
            bytes_used: U64::new(0),
            change_time: U64::new(change_time),
            warning_threshold: I64::new(QUOTA_UNLIMITED),
            hard_limit: I64::new(QUOTA_UNLIMITED),
            peak_quota: U64::new(0),
        }
    }
}

/// Quota owner entry data ($Quota:$O data)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C)]
pub struct QuotaOEntryData {
    pub owner_id: U32,
    pub unknown: U32,
}

impl QuotaOEntryData {
    pub const fn new(owner_id: u32, unknown: u32) -> Self {
        Self {
            owner_id: U32::new(owner_id),
            unknown: U32::new(unknown),
        }
    }
}

const _: () = {
    assert!(core::mem::size_of::<QuotaQData>() == 48);
    assert!(core::mem::align_of::<QuotaQData>() == 1);
    assert!(core::mem::offset_of!(QuotaQData, warning_threshold) == 24);
    assert!(core::mem::size_of::<QuotaOEntryData>() == 8);
};
#[cfg(test)]
mod endian_tests {
    use super::*;
    #[test]
    fn unlimited_quota_keeps_signed_little_endian_sentinel() {
        let quota = QuotaQData::new_unlimited(0x12345678, 0x1122334455667788);
        assert_eq!(&quota.as_bytes()[4..8], &[0x78, 0x56, 0x34, 0x12]);
        assert_eq!(
            &quota.as_bytes()[16..24],
            &[0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11]
        );
        assert_eq!(&quota.as_bytes()[24..40], &[0xff; 16]);
        assert_eq!(
            QuotaQData::ref_from_bytes(quota.as_bytes())
                .unwrap()
                .hard_limit
                .get(),
            -1
        );
    }
}
