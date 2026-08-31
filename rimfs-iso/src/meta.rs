// SPDX-License-Identifier: MIT

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{string::String, vec::Vec};

use crate::types::{ISO_SECTOR_SIZE, IsoHandle};
use rimfs_core::meta::FsMeta;

/// Configuration and metadata for ISO 9660 image generation and inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoMeta {
    pub volume_id: String,
    pub publisher: String,
    pub application_id: String,
    pub enable_joliet: bool,
    pub enable_rock_ridge: bool,
    pub boot_efi: Option<Vec<u8>>,
    pub boot_bios: Option<Vec<u8>>,
    pub total_size: u64,
}

impl Default for IsoMeta {
    fn default() -> Self {
        Self {
            volume_id: String::from("RIM_ISO"),
            publisher: String::from("RIM"),
            application_id: String::from("RIM STORAGE SYNTHESIZER"),
            enable_joliet: true,
            enable_rock_ridge: true,
            boot_efi: None,
            boot_bios: None,
            total_size: 0,
        }
    }
}

impl FsMeta<IsoHandle> for IsoMeta {
    #[inline]
    fn unit_size(&self) -> usize {
        ISO_SECTOR_SIZE
    }

    #[inline]
    fn unit_offset(&self, unit: IsoHandle) -> u64 {
        unit.0 * (ISO_SECTOR_SIZE as u64)
    }

    #[inline]
    fn root_unit(&self) -> IsoHandle {
        IsoHandle(16) // PVD sector
    }

    #[inline]
    fn first_data_unit(&self) -> IsoHandle {
        IsoHandle(20)
    }

    #[inline]
    fn last_data_unit(&self) -> IsoHandle {
        IsoHandle(self.total_size.saturating_sub(1) / (ISO_SECTOR_SIZE as u64))
    }

    #[inline]
    fn total_units(&self) -> usize {
        (self.total_size as usize) / ISO_SECTOR_SIZE
    }

    #[inline]
    fn size_bytes(&self) -> u64 {
        self.total_size
    }

    #[inline]
    fn label(&self) -> String {
        self.volume_id.clone()
    }
}

impl IsoMeta {
    pub fn new(total_size: u64, volume_id: Option<&str>) -> rimfs_core::FsResult<Self> {
        Ok(Self {
            volume_id: String::from(volume_id.unwrap_or("RIM_ISO")),
            publisher: String::from("RIM"),
            application_id: String::from("RIM STORAGE SYNTHESIZER"),
            enable_joliet: true,
            enable_rock_ridge: true,
            boot_efi: None,
            boot_bios: None,
            total_size,
        })
    }
}
