// SPDX-License-Identifier: MIT

//! Empty TAR archive initialization.

use crate::meta::TarMeta;
use rimfs_core::formatter::{FsFormatter, FsFormatterResult};
use rimio::RimIO;

/// Initializes an empty TAR archive (writes standard 1024-byte zero trailer).
pub struct TarFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a TarMeta,
}

impl<'a, IO: RimIO + ?Sized> TarFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a TarMeta) -> Self {
        Self { io, _meta: meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for TarFormatter<'a, IO> {
    fn format(&mut self, _full_format: bool) -> FsFormatterResult {
        self.io.zero_at(0, 1024)?;
        self.io.flush()?;
        Ok(())
    }
}
