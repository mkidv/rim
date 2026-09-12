// SPDX-License-Identifier: MIT

//! Supported partition filesystem format specifications.

use crate::errors::{LayoutError, LayoutResult};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    Fat32,
    Fat16,
    Fat12,
    Fat8,
    RimFat,
    ExFat,
    Ntfs,
    Ext4,
    Btrfs,
    Xfs,
    Raw,
    None,
}

impl Filesystem {
    pub fn check_size_limit(&self, size_mb: u64) -> LayoutResult<()> {
        match self {
            Filesystem::Fat32 if size_mb > 32 * 1024 => Err(LayoutError::SizeTooLarge {
                fs: *self,
                size_mb,
                limit_mb: 32 * 1024,
            }),
            Filesystem::Fat16 if size_mb > 2 * 1024 => Err(LayoutError::SizeTooLarge {
                fs: *self,
                size_mb,
                limit_mb: 2 * 1024,
            }),
            Filesystem::Fat12 if size_mb > 32 => Err(LayoutError::SizeTooLarge {
                fs: *self,
                size_mb,
                limit_mb: 32,
            }),
            Filesystem::ExFat if size_mb < 32 => Err(LayoutError::SizeTooSmall {
                fs: *self,
                size_mb,
                min_mb: 32,
            }),
            Filesystem::Ext4 if size_mb < 16 => Err(LayoutError::SizeTooSmall {
                fs: *self,
                size_mb,
                min_mb: 16,
            }),
            Filesystem::Ntfs if size_mb < 4 => Err(LayoutError::SizeTooSmall {
                fs: *self,
                size_mb,
                min_mb: 4,
            }),
            Filesystem::Btrfs if size_mb < 64 => Err(LayoutError::SizeTooSmall {
                fs: *self,
                size_mb,
                min_mb: 64,
            }),
            Filesystem::Xfs if size_mb < 300 => Err(LayoutError::SizeTooSmall {
                fs: *self,
                size_mb,
                min_mb: 300,
            }),
            _ => Ok(()),
        }
    }

    pub fn validate(&self) -> LayoutResult<()> {
        Ok(())
    }
}

impl core::fmt::Display for Filesystem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Filesystem::Fat32 => "FAT32",
            Filesystem::Fat16 => "FAT16",
            Filesystem::Fat12 => "FAT12",
            Filesystem::Fat8 => "FAT8",
            Filesystem::RimFat => "RimFAT",
            Filesystem::ExFat => "exFAT",
            Filesystem::Ntfs => "NTFS",
            Filesystem::Ext4 => "ext4",
            Filesystem::Btrfs => "btrfs",
            Filesystem::Xfs => "xfs",
            Filesystem::Raw => "raw",
            Filesystem::None => "none",
        };
        write!(f, "{s}")
    }
}
