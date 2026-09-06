// SPDX-License-Identifier: MIT
//! NTFS Quota ($Quota) structures and constants
//!
//! Reference: Microsoft NTFS on-disk format & ntfs-3g

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

pub const QUOTA_VERSION_2: u32 = 2;
pub const QUOTA_OWNER_ID_DEFAULT: u32 = 1;
pub const QUOTA_OWNER_ID_ADMINS: u32 = 256;
pub const QUOTA_UNLIMITED: i64 = -1;

/// Quota user control block ($Quota:$Q data)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct QuotaQData {
    /// Quota record version (always 2 on modern NTFS)
    pub version: u32,
    /// Quota flags (e.g. DEFAULT_LIMITS)
    pub flags: u32,
    /// Number of bytes used by this owner
    pub bytes_used: u64,
    /// Last change timestamp in FILETIME format
    pub change_time: u64,
    /// Soft quota warning threshold (-1 for unlimited)
    pub warning_threshold: i64,
    /// Hard quota limit (-1 for unlimited)
    pub hard_limit: i64,
    /// Peak quota usage reached
    pub peak_quota: u64,
}

impl QuotaQData {
    pub const fn new_unlimited(flags: u32, change_time: u64) -> Self {
        Self {
            version: QUOTA_VERSION_2,
            flags,
            bytes_used: 0,
            change_time,
            warning_threshold: QUOTA_UNLIMITED,
            hard_limit: QUOTA_UNLIMITED,
            peak_quota: 0,
        }
    }
}

/// Quota owner entry data ($Quota:$O data)
#[derive(Debug, Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(C, packed)]
pub struct QuotaOEntryData {
    pub owner_id: u32,
    pub unknown: u32,
}

impl QuotaOEntryData {
    pub const fn new(owner_id: u32, unknown: u32) -> Self {
        Self { owner_id, unknown }
    }
}
