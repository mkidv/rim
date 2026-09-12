// SPDX-License-Identifier: MIT

//! TAR archive metadata and block size constants.

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::String;

use crate::types::TAR_BLOCK_SIZE;
use rimfs_core::meta::FsMeta;

/// Configuration and metadata for TAR archive filesystem operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TarMeta {
    pub block_size: u64,
    pub total_size: u64,
    pub label: String,
}

impl Default for TarMeta {
    fn default() -> Self {
        Self {
            block_size: TAR_BLOCK_SIZE as u64,
            total_size: 0,
            label: String::from("TARFS"),
        }
    }
}

impl FsMeta<u64> for TarMeta {
    #[inline]
    fn unit_size(&self) -> u64 {
        self.block_size
    }

    #[inline]
    fn unit_offset(&self, unit: u64) -> u64 {
        unit * self.block_size
    }

    #[inline]
    fn root_unit(&self) -> u64 {
        0
    }

    #[inline]
    fn first_data_unit(&self) -> u64 {
        0
    }

    #[inline]
    fn last_data_unit(&self) -> u64 {
        self.total_size.saturating_sub(1) / self.block_size
    }

    #[inline]
    fn total_units(&self) -> u64 {
        self.total_size / self.block_size
    }

    #[inline]
    fn size_bytes(&self) -> u64 {
        self.total_size
    }

    #[inline]
    fn label(&self) -> String {
        self.label.clone()
    }
}

impl TarMeta {
    pub fn new(total_size: u64, label: Option<&str>) -> rimfs_core::FsResult<Self> {
        Ok(Self {
            block_size: TAR_BLOCK_SIZE as u64,
            total_size,
            label: String::from(label.unwrap_or("TARFS")),
        })
    }
}
