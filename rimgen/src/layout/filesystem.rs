// SPDX-License-Identifier: MIT

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    Fat32,
    ExFat,
    Ntfs,
    Ext4,
    Btrfs,
    Xfs,
    Raw,
    None,
}

impl Filesystem {
    pub fn check_size_limit(&self, size_mb: u64) -> anyhow::Result<()> {
        match self {
            Filesystem::Fat32 if size_mb > 32 * 1024 => {
                anyhow::bail!(
                    "FAT32 is not recommended beyond 32 GiB (got {} MiB)",
                    size_mb
                );
            }
            Filesystem::ExFat if size_mb < 256 => {
                anyhow::bail!(
                    "exFAT is not recommended under 256 MiB (got {} MiB)",
                    size_mb
                );
            }
            Filesystem::Ext4 if size_mb < 16 => {
                anyhow::bail!("ext4 needs at least 16 MiB (got {} MiB)", size_mb);
            }
            Filesystem::Btrfs if size_mb < 64 => {
                anyhow::bail!("btrfs needs at least 64 MiB (got {} MiB)", size_mb);
            }
            Filesystem::Xfs if size_mb < 300 => {
                anyhow::bail!(
                    "xfs typically requires at least 300 MiB (got {} MiB)",
                    size_mb
                );
            }
            _ => Ok(()),
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

impl core::fmt::Display for Filesystem {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Filesystem::Fat32 => "FAT32",
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
