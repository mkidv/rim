// SPDX-License-Identifier: MIT

//! ZIP archive metadata and alignment parameters.

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::String;

use crate::types::{METHOD_STORE, ZipHandle};
use rimfs_core::meta::FsMeta;

/// Configuration and metadata for ZIP archive operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZipMeta {
    pub compression_method: u16,
    pub total_size: u64,
    pub label: String,
}

impl Default for ZipMeta {
    fn default() -> Self {
        Self {
            compression_method: METHOD_STORE,
            total_size: 0,
            label: String::from("ZIPFS"),
        }
    }
}

impl FsMeta<u64> for ZipMeta {
    #[inline]
    fn unit_size(&self) -> u64 {
        1
    }

    #[inline]
    fn unit_offset(&self, unit: u64) -> u64 {
        unit
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
        self.total_size.saturating_sub(1)
    }

    #[inline]
    fn total_units(&self) -> u64 {
        self.total_size
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

impl FsMeta<ZipHandle> for ZipMeta {
    #[inline]
    fn unit_size(&self) -> u64 {
        1
    }

    #[inline]
    fn unit_offset(&self, unit: ZipHandle) -> u64 {
        unit.0
    }

    #[inline]
    fn root_unit(&self) -> ZipHandle {
        ZipHandle(0)
    }

    #[inline]
    fn first_data_unit(&self) -> ZipHandle {
        ZipHandle(0)
    }

    #[inline]
    fn last_data_unit(&self) -> ZipHandle {
        ZipHandle(self.total_size.saturating_sub(1))
    }

    #[inline]
    fn total_units(&self) -> u64 {
        self.total_size
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

impl ZipMeta {
    /// Read and validate an existing archive's directory without modifying storage.
    /// Compression is per entry; METHOD_STORE remains the default for new writes.
    /// This does not turn a new injector into an incremental archive editor.
    pub fn from_io<IO: rimio::RimRead + ?Sized>(io: &mut IO) -> rimfs_core::FsResult<Self> {
        let total_size = io.total_size()?;
        let meta = Self::new(total_size, None)?;
        crate::resolver::ZipResolver::try_new(io, &meta)?;
        Ok(meta)
    }

    pub fn new(total_size: u64, label: Option<&str>) -> rimfs_core::FsResult<Self> {
        Ok(Self {
            compression_method: METHOD_STORE,
            total_size,
            label: String::from(label.unwrap_or("ZIPFS")),
        })
    }
}
