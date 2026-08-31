// SPDX-License-Identifier: MIT

use crate::meta::ZipMeta;
use crate::types::{END_OF_CENTRAL_DIR_FIXED_SIZE, END_OF_CENTRAL_DIR_SIG};
use rimfs_core::errors::FsFormatterResult;
use rimfs_core::formatter::FsFormatter;
use rimio::RimIO;

/// Initializes and writes an empty ZIP archive structure.
pub struct ZipFormatter<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    _meta: &'a ZipMeta,
}

impl<'a, IO: RimIO + ?Sized> ZipFormatter<'a, IO> {
    pub fn new(io: &'a mut IO, meta: &'a ZipMeta) -> Self {
        Self { io, _meta: meta }
    }
}

impl<'a, IO: RimIO + ?Sized> FsFormatter for ZipFormatter<'a, IO> {
    fn format(&mut self, _is_quick: bool) -> FsFormatterResult {
        // Write an empty End of Central Directory (EOCD) record
        let mut eocd = [0u8; END_OF_CENTRAL_DIR_FIXED_SIZE];
        eocd[0..4].copy_from_slice(&END_OF_CENTRAL_DIR_SIG.to_le_bytes());
        // disk_num = 0, cd_disk = 0, entries_disk = 0, total_entries = 0, cd_size = 0, cd_offset = 0, comment_len = 0
        self.io.write_at(0, &eocd)?;
        self.io.flush()?;
        Ok(())
    }
}
