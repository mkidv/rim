// SPDX-License-Identifier: MIT
#![cfg(feature = "uefi")]

use crate::{BlockIO, BlockIOError, BlockIOResult};
use uefi::ResultExt;
use uefi::proto::block_io::BlockIO;
use uefi::proto::block_io::Media;
use uefi::table::boot::ScopedProtocol;

/// UEFI BlockIO backend for `BlockIO` trait.
pub struct UefiBlockIO<'a> {
    block_io: ScopedProtocol<'a, BlockIO>,
    media: &'a Media,
    partition_offset: u64,
}

impl<'a> UefiBlockIO<'a> {
    /// Creates a new `UefiBlockIO` from a located BlockIO protocol.
    ///
    /// You should call `boot_services.locate_protocol::<BlockIO>()` first.
    pub fn new(block_io: ScopedProtocol<'a, BlockIO>) -> Self {
        let media = unsafe { &*block_io.media() };
        Self {
            block_io,
            media,
            partition_offset: 0,
        }
    }

    pub fn new_with_offset(block_io: ScopedProtocol<'a, BlockIO>, partition_offset: u64) -> Self {
        let media = unsafe { &*block_io.media() };
        Self {
            block_io,
            media,
            partition_offset,
        }
    }

    pub fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    pub fn partition_offset(&self) -> u64 {
        self.partition_offset
    }

    fn block_size(&self) -> u64 {
        self.media.block_size() as u64
    }

    fn check_bounds(&self, offset: u64, len: usize) -> BlockIOResult {
        let end = offset + len as u64;
        let media_size = self.media.last_block() * self.block_size() + self.block_size();
        if end > media_size {
            return Err(BlockIOError::OutOfBounds);
        }
        Ok(())
    }
}

impl<'a> BlockIO for UefiBlockIO<'a> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> BlockIOResult {
        let abs_offset = self.partition_offset + offset;

        self.check_bounds(abs_offset, data.len())?;

        let block_size = self.block_size();
        let mut remaining = data;
        let mut current_offset = abs_offset;

        while !remaining.is_empty() {
            let lba = current_offset / block_size;
            let lba_offset = (current_offset % block_size) as usize;
            let to_write = (block_size as usize - lba_offset).min(remaining.len());

            // Read current block (for RMW)
            let mut block_buf = [0u8; 512]; // BlockIO spec says block_size <= 512 is common
            if block_size > 512 {
                return Err(BlockIOError::Unsupported);
            }

            self.block_io
                .read_blocks(
                    self.media.media_id(),
                    lba,
                    &mut block_buf[..block_size as usize],
                )
                .log_err()
                .map_err(|_| BlockIOError::Error("UEFI read_blocks failed"))?;

            // Overwrite portion
            block_buf[lba_offset..lba_offset + to_write].copy_from_slice(&remaining[..to_write]);

            // Write back block
            self.block_io
                .write_blocks(
                    self.media.media_id(),
                    lba,
                    &block_buf[..block_size as usize],
                )
                .log_err()
                .map_err(|_| BlockIOError::Error("UEFI write_blocks failed"))?;

            current_offset += to_write as u64;
            remaining = &remaining[to_write..];
        }

        Ok(())
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> BlockIOResult {
        let abs_offset = self.partition_offset + offset;

        self.check_bounds(abs_offset, buf.len())?;

        let block_size = self.block_size();
        let mut remaining = buf;
        let mut current_offset = abs_offset;

        while !remaining.is_empty() {
            let lba = current_offset / block_size;
            let lba_offset = (current_offset % block_size) as usize;
            let to_read = (block_size as usize - lba_offset).min(remaining.len());

            let mut block_buf = [0u8; 512];
            if block_size > 512 {
                return Err(BlockIOError::Other(
                    "Unsupported block size > 512".to_string(),
                ));
            }

            self.block_io
                .read_blocks(
                    self.media.media_id(),
                    lba,
                    &mut block_buf[..block_size as usize],
                )
                .log_err()
                .map_err(|_| BlockIOError::Error("UEFI read_blocks failed"))?;

            remaining[..to_read].copy_from_slice(&block_buf[lba_offset..lba_offset + to_read]);

            current_offset += to_read as u64;
            remaining = &mut remaining[to_read..];
        }

        Ok(())
    }

    fn flush(&mut self) -> BlockIOResult {
        self.block_io
            .flush_blocks()
            .log_err()
            .map_err(|_| BlockIOError::Error("UEFI flush_blocks failed"))
    }
}
