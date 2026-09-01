// SPDX-License-Identifier: MIT

use rimio::errors::RimIOError;
use rimio::prelude::*;

use crate::errors::{RimImgError, RimImgResult};
use crate::format::ImageFormat;
use crate::options::ImageOptions;
use crate::{qcow2, vdi, vhd, vmdk};

/// Logical raw-disk view over an image container.
#[allow(clippy::upper_case_acronyms)]
pub struct ImageIO<'a> {
    inner: &'a mut dyn RimIO,
    raw_len: u64,
    data_offset: u64,
    format: ImageFormat,
    options: ImageOptions,
    partition_offset: u64,
    write_footer_on_finish: bool,
    finished: bool,
}

/// Read-only logical raw-disk view over an image container.
#[allow(clippy::upper_case_acronyms)]
pub struct ImageReadIO<'a> {
    inner: &'a mut dyn RimRead,
    raw_len: u64,
    data_offset: u64,
    format: ImageFormat,
    partition_offset: u64,
}

impl<'a> ImageIO<'a> {
    fn new(
        inner: &'a mut dyn RimIO,
        raw_len: u64,
        data_offset: u64,
        format: ImageFormat,
        options: ImageOptions,
        write_footer_on_finish: bool,
    ) -> Self {
        Self {
            inner,
            raw_len,
            data_offset,
            format,
            options,
            partition_offset: 0,
            write_footer_on_finish,
            finished: false,
        }
    }

    pub fn raw_len(&self) -> u64 {
        self.raw_len
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    pub fn finish(&mut self) -> RimImgResult {
        if self.finished {
            return Ok(());
        }

        if self.write_footer_on_finish {
            finish_vhd(self.inner, self.raw_len, self.options)?;
        }

        self.inner.flush()?;
        self.finished = true;
        Ok(())
    }

    #[inline]
    fn checked_raw_range(&self, offset: u64, len: usize) -> Result<u64, RimIOError> {
        let logical = self
            .partition_offset
            .checked_add(offset)
            .ok_or(RimIOError::OutOfBounds)?;
        let end = logical
            .checked_add(len as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.raw_len {
            return Err(RimIOError::OutOfBounds);
        }

        self.data_offset
            .checked_add(logical)
            .ok_or(RimIOError::OutOfBounds)
    }
}

impl<'a> ImageReadIO<'a> {
    fn new(
        inner: &'a mut dyn RimRead,
        raw_len: u64,
        data_offset: u64,
        format: ImageFormat,
    ) -> Self {
        Self {
            inner,
            raw_len,
            data_offset,
            format,
            partition_offset: 0,
        }
    }

    pub fn raw_len(&self) -> u64 {
        self.raw_len
    }

    pub fn format(&self) -> ImageFormat {
        self.format
    }

    #[inline]
    fn checked_raw_range(&self, offset: u64, len: usize) -> Result<u64, RimIOError> {
        let logical = self
            .partition_offset
            .checked_add(offset)
            .ok_or(RimIOError::OutOfBounds)?;
        let end = logical
            .checked_add(len as u64)
            .ok_or(RimIOError::OutOfBounds)?;
        if end > self.raw_len {
            return Err(RimIOError::OutOfBounds);
        }

        self.data_offset
            .checked_add(logical)
            .ok_or(RimIOError::OutOfBounds)
    }
}

impl RimRead for ImageReadIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        let physical = self.checked_raw_range(offset, buf.len())?;
        self.inner.read_at(physical, buf)
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        Ok(self.raw_len.saturating_sub(self.partition_offset))
    }
}

impl RimRead for ImageIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        let physical = self.checked_raw_range(offset, buf.len())?;
        self.inner.read_at(physical, buf)
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        Ok(self.raw_len.saturating_sub(self.partition_offset))
    }
}

impl RimWrite for ImageIO<'_> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> rimio::RimIOResult {
        let physical = self.checked_raw_range(offset, data.len())?;
        self.inner.write_at(physical, data)
    }

    fn flush(&mut self) -> rimio::RimIOResult {
        self.inner.flush()
    }
}

impl RimIO for ImageIO<'_> {
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    fn partition_offset(&self) -> u64 {
        self.partition_offset
    }
}

#[cfg(feature = "alloc")]
pub fn create_image_io<'a>(
    dst: &'a mut dyn RimIO,
    raw_len: u64,
    format: ImageFormat,
    options: ImageOptions,
) -> RimImgResult<ImageIO<'a>> {
    let (data_offset, write_footer_on_finish) = match format {
        ImageFormat::Raw => (0, false),
        ImageFormat::Vhd => (0, true),
        ImageFormat::Vmdk => {
            vmdk::init_vmdk_io(dst, raw_len, options)?;
            (vmdk::DESCRIPTOR_SECTORS * vmdk::SECTOR_SIZE, false)
        }
        ImageFormat::Qcow2 => (qcow2::init_qcow2_io(dst, raw_len)?, false),
        ImageFormat::Vdi => {
            vdi::init_vdi_io(dst, raw_len, options)?;
            (vdi::DATA_OFFSET, false)
        }
    };

    Ok(ImageIO::new(
        dst,
        raw_len,
        data_offset,
        format,
        options,
        write_footer_on_finish,
    ))
}

pub fn open_image_io(src: &mut dyn RimIO) -> RimImgResult<ImageIO<'_>> {
    let format = ImageFormat::from_io(src)?;
    let (raw_len, data_offset) = match format {
        ImageFormat::Raw => (src.total_size()?, 0),
        ImageFormat::Vhd => open_vhd(src)?,
        ImageFormat::Vmdk => open_vmdk(src)?,
        ImageFormat::Qcow2 => open_qcow2(src)?,
        ImageFormat::Vdi => open_vdi(src)?,
    };

    Ok(ImageIO::new(
        src,
        raw_len,
        data_offset,
        format,
        ImageOptions::deterministic(0),
        false,
    ))
}

pub fn open_image_read_io(src: &mut dyn RimRead) -> RimImgResult<ImageReadIO<'_>> {
    let format = ImageFormat::from_read(src)?;
    let (raw_len, data_offset) = match format {
        ImageFormat::Raw => (src.total_size()?, 0),
        ImageFormat::Vhd => open_vhd(src)?,
        ImageFormat::Vmdk => open_vmdk(src)?,
        ImageFormat::Qcow2 => open_qcow2(src)?,
        ImageFormat::Vdi => open_vdi(src)?,
    };

    Ok(ImageReadIO::new(src, raw_len, data_offset, format))
}

fn finish_vhd(dst: &mut dyn RimIO, raw_len: u64, options: ImageOptions) -> RimImgResult {
    let mut total_size = raw_len;
    let remainder = total_size % vhd::VHD_FOOTER_SIZE;
    if remainder != 0 {
        let pad_size = usize::try_from(vhd::VHD_FOOTER_SIZE - remainder)
            .map_err(|_| RimImgError::SizeOverflow)?;
        dst.zero_fill(total_size, pad_size)?;
        total_size += pad_size as u64;
    }

    let footer = vhd::VhdFooter::new_fixed(total_size, options);
    dst.write_struct(total_size, &footer)?;
    Ok(())
}

fn open_vhd(src: &mut dyn RimRead) -> RimImgResult<(u64, u64)> {
    let len = src.total_size()?;
    if len < vhd::VHD_FOOTER_SIZE {
        return Err(RimImgError::InvalidHeader(
            "VHD file too small (less than 512 bytes)",
        ));
    }

    let footer: vhd::VhdFooter = src.read_struct(len - vhd::VHD_FOOTER_SIZE)?;
    if !footer.validate() {
        return Err(RimImgError::Corrupted("Invalid VHD footer"));
    }

    Ok((len - vhd::VHD_FOOTER_SIZE, 0))
}

fn open_vmdk(src: &mut dyn RimRead) -> RimImgResult<(u64, u64)> {
    let len = src.total_size()?;
    let data_offset = vmdk::DESCRIPTOR_SECTORS * vmdk::SECTOR_SIZE;
    if len < data_offset {
        return Err(RimImgError::InvalidHeader(
            "VMDK file too small (header truncated)",
        ));
    }

    Ok((len - data_offset, data_offset))
}

fn open_qcow2(src: &mut dyn RimRead) -> RimImgResult<(u64, u64)> {
    let header: qcow2::Qcow2Header = src.read_struct(0)?;
    qcow2::validate_qcow2_header(&header)?;
    let data_offset = qcow2::data_start_from_header(&header);
    Ok((header.size.get(), data_offset))
}

fn open_vdi(src: &mut dyn RimRead) -> RimImgResult<(u64, u64)> {
    let header: vdi::VdiHeader = src.read_struct(64)?;
    vdi::validate_vdi_header(&header)?;
    Ok((header.disk_size.get(), header.offset_data.get() as u64))
}
