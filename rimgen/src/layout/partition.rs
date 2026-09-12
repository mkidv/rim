// SPDX-License-Identifier: MIT

//! Partition configuration and size calculation.

use crate::errors::{LayoutError, LayoutResult};
use crate::layout::filesystem::Filesystem;
use crate::layout::size::Size;
#[cfg(not(feature = "std"))]
use alloc::string::String;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct PartitionConfig {
    pub name: String,
    #[serde(rename = "type", alias = "partition_type", alias = "partition-type")]
    pub kind: Option<PartitionKind>,
    #[serde(default, alias = "mount_point", alias = "mount-point")]
    pub mountpoint: Option<String>,
    pub size: Size,
    #[serde(alias = "filesystem")]
    pub fs: Filesystem,
    #[serde(default)]
    pub bootable: bool,
    #[serde(default)]
    pub guid: Option<uuid::Uuid>,
    pub index: Option<usize>,
    #[cfg(feature = "std")]
    #[serde(default)]
    pub payload: Option<std::path::PathBuf>,
    #[cfg(not(feature = "std"))]
    #[serde(default)]
    pub payload: Option<String>,
    pub label: Option<String>,
    pub uuid: Option<String>,
}

impl PartitionConfig {
    pub fn effective_kind(&self) -> PartitionKind {
        self.kind
            .unwrap_or_else(|| PartitionKind::default_for_fs(&self.fs, self.bootable))
    }

    pub fn is_mountable(&self) -> bool {
        !matches!(self.fs, Filesystem::Raw | Filesystem::None)
    }

    pub fn validate(&self) -> LayoutResult<()> {
        crate::ensure!(
            !matches!(self.fs, Filesystem::Raw | Filesystem::None) || self.mountpoint.is_none(),
            LayoutError::MountpointOnNonMountable {
                name: self.name.clone(),
                fs: self.fs,
            }
        );

        if let Size::Fixed(size_mb) = self.size {
            self.fs.check_size_limit(size_mb)?;
        }

        self.fs.validate()?;

        crate::ensure!(
            !self.is_mountable() || self.guid.is_some(),
            LayoutError::MissingGuid(self.name.clone())
        );

        crate::ensure!(
            !self.effective_kind().requires_explicit() || self.kind.is_some(),
            LayoutError::RequiresExplicitKind {
                name: self.name.clone(),
                kind: self.effective_kind(),
            }
        );

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PartitionKind {
    Esp,
    Data,
    Linux,
    Biosboot,
    Swap,
    Boot,
    Recovery,
}

impl PartitionKind {
    pub fn requires_explicit(&self) -> bool {
        matches!(
            self,
            PartitionKind::Biosboot | PartitionKind::Swap | PartitionKind::Recovery
        )
    }

    pub fn default_for_fs(fs: &Filesystem, bootable: bool) -> Self {
        match fs {
            Filesystem::Fat32 | Filesystem::Fat16 | Filesystem::Fat12 | Filesystem::Fat8 => {
                if bootable {
                    PartitionKind::Esp
                } else {
                    PartitionKind::Data
                }
            }
            Filesystem::RimFat => {
                if bootable {
                    PartitionKind::Esp
                } else {
                    PartitionKind::Data
                }
            }
            Filesystem::Ext4 | Filesystem::Btrfs | Filesystem::Xfs => PartitionKind::Linux,
            Filesystem::Ntfs | Filesystem::ExFat => PartitionKind::Data,
            Filesystem::Raw | Filesystem::None => PartitionKind::Biosboot,
        }
    }
}
