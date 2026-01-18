// SPDX-License-Identifier: MIT

use crate::layout::filesystem::Filesystem;
use crate::layout::size::Size;
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct Partition {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: Option<PartitionKind>,
    #[serde(default)]
    pub mountpoint: Option<String>,
    pub size: Size,
    pub fs: Filesystem,
    #[serde(default)]
    pub bootable: bool,
    #[serde(default)]
    pub guid: Option<uuid::Uuid>,
    pub index: Option<usize>,
    #[serde(default)]
    pub payload: Option<std::path::PathBuf>,
    pub label: Option<String>,
    pub uuid: Option<String>,
}

impl Partition {
    pub fn effective_kind(&self) -> PartitionKind {
        self.kind
            .clone()
            .unwrap_or_else(|| PartitionKind::default_for_fs(&self.fs, self.bootable))
    }

    pub fn is_mountable(&self) -> bool {
        !matches!(self.fs, Filesystem::Raw | Filesystem::None)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if matches!(self.fs, Filesystem::Raw | Filesystem::None) {
            if self.mountpoint.is_some() {
                anyhow::bail!(
                    "Partition '{}' is marked as Raw/None but has a mountpoint '{}'",
                    self.name,
                    self.mountpoint.as_deref().unwrap_or("")
                );
            }
        } else if self.payload.is_some() {
            anyhow::bail!(
                "Partition '{}' has a 'payload' defined but is not of type 'raw' (fs={}). Payload is only for raw partitions.",
                self.name,
                self.fs
            );
        }

        if self.payload.is_some() && self.mountpoint.is_some() {
            anyhow::bail!(
                "Partition '{}' cannot have both 'mountpoint' and 'payload'.",
                self.name
            );
        }

        if let Size::Fixed(mb) = self.size {
            self.fs.check_size_limit(mb)?;
        }

        if let Size::Auto = self.size {
            anyhow::bail!(
                "Partition '{}' still has size = 'auto' at validation step.",
                self.name
            );
        }

        self.fs.validate()?;

        if self.is_mountable() && self.guid.is_none() {
            anyhow::bail!(
                "Partition '{}' is mountable but has no GUID assigned. Did you forget to call `assign_guids()`?",
                self.name
            );
        }

        if self.name.len() > 36 {
            crate::log_verbose!(
                "Warning: partition name '{}' truncated to 36 characters",
                self.name
            );
        }

        if !self.name.is_ascii() {
            crate::log_verbose!(
                "Warning: ASCII character of partition name '{}' will be ignored",
                self.name
            );
        }

        let expected_kind = PartitionKind::default_for_fs(&self.fs, self.bootable);
        let effective_kind = self.effective_kind();

        if self.effective_kind().requires_explicit() && self.kind.is_none() {
            anyhow::bail!(
                "Partition '{}' requires explicit type '{:?}' but none was provided.",
                self.name,
                self.effective_kind()
            );
        }

        if self.kind.is_some() && effective_kind != expected_kind {
            crate::log_verbose!(
                "Warning: Partition '{}' uses fs '{}' but type '{:?}' (expected {:?})",
                self.name,
                self.fs,
                effective_kind,
                expected_kind
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
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
            Filesystem::Fat32 => {
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
