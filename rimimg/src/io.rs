// SPDX-License-Identifier: MIT

use rimio::errors::RimIOError;
use rimio::prelude::*;

use crate::errors::{RimImgError, RimImgResult};
use crate::format::ImageFormat;
use crate::options::ImageOptions;
use crate::{qcow2, vdi, vhd, vmdk};

/// Linear raw-disk view over an image container (RAW, VHD, VMDK, VDI).
#[allow(clippy::upper_case_acronyms)]
pub struct LinearImageIO<'a> {
    inner: &'a mut dyn RimIO,
    raw_len: u64,
    data_offset: u64,
    format: ImageFormat,
    options: ImageOptions,
    partition_offset: u64,
    write_footer_on_finish: bool,
    finished: bool,
}

/// Read-only linear raw-disk view over an image container (RAW, VHD, VMDK, VDI).
#[allow(clippy::upper_case_acronyms)]
pub struct LinearImageReadIO<'a> {
    inner: &'a mut dyn RimRead,
    raw_len: u64,
    data_offset: u64,
    format: ImageFormat,
    partition_offset: u64,
}

/// Logical raw-disk view over an image container.
#[allow(clippy::upper_case_acronyms)]
pub enum ImageIO<'a> {
    Linear(LinearImageIO<'a>),
    #[cfg(feature = "alloc")]
    Qcow2(qcow2::Qcow2IO<'a>),
}

/// Read-only logical raw-disk view over an image container.
#[allow(clippy::upper_case_acronyms)]
pub enum ImageReadIO<'a> {
    Linear(LinearImageReadIO<'a>),
    Qcow2(qcow2::Qcow2ReadIO<'a>),
}

impl<'a> LinearImageIO<'a> {
    pub fn new(
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

impl<'a> LinearImageReadIO<'a> {
    pub fn new(
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

    pub fn set_offset(&mut self, offset: u64) -> u64 {
        self.partition_offset = offset;
        offset
    }

    pub fn partition_offset(&self) -> u64 {
        self.partition_offset
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

impl<'a> ImageIO<'a> {
    pub fn raw_len(&self) -> u64 {
        match self {
            Self::Linear(io) => io.raw_len(),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.raw_len(),
        }
    }

    pub fn format(&self) -> ImageFormat {
        match self {
            Self::Linear(io) => io.format(),
            #[cfg(feature = "alloc")]
            Self::Qcow2(_) => ImageFormat::Qcow2,
        }
    }

    pub fn finish(&mut self) -> RimImgResult {
        match self {
            Self::Linear(io) => io.finish(),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.finish(),
        }
    }
}

impl<'a> ImageReadIO<'a> {
    pub fn raw_len(&self) -> u64 {
        match self {
            Self::Linear(io) => io.raw_len(),
            Self::Qcow2(io) => io.raw_len(),
        }
    }

    pub fn format(&self) -> ImageFormat {
        match self {
            Self::Linear(io) => io.format(),
            Self::Qcow2(_) => ImageFormat::Qcow2,
        }
    }

    pub fn set_offset(&mut self, offset: u64) -> u64 {
        match self {
            Self::Linear(io) => io.set_offset(offset),
            Self::Qcow2(io) => io.set_offset(offset),
        }
    }

    pub fn partition_offset(&self) -> u64 {
        match self {
            Self::Linear(io) => io.partition_offset(),
            Self::Qcow2(io) => io.partition_offset(),
        }
    }
}

impl RimRead for LinearImageReadIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        let physical = self.checked_raw_range(offset, buf.len())?;
        self.inner.read_at(physical, buf)
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        Ok(self.raw_len.saturating_sub(self.partition_offset))
    }
}

impl RimRead for LinearImageIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        let physical = self.checked_raw_range(offset, buf.len())?;
        self.inner.read_at(physical, buf)
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        Ok(self.raw_len.saturating_sub(self.partition_offset))
    }
}

impl RimWrite for LinearImageIO<'_> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> rimio::RimIOResult {
        let physical = self.checked_raw_range(offset, data.len())?;
        self.inner.write_at(physical, data)
    }

    fn flush(&mut self) -> rimio::RimIOResult {
        self.inner.flush()
    }
}

impl RimIO for LinearImageIO<'_> {
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        self.partition_offset = partition_offset;
        partition_offset
    }

    fn partition_offset(&self) -> u64 {
        self.partition_offset
    }
}

impl RimRead for ImageReadIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        match self {
            Self::Linear(io) => io.read_at(offset, buf),
            Self::Qcow2(io) => io.read_at(offset, buf),
        }
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        match self {
            Self::Linear(io) => io.total_size(),
            Self::Qcow2(io) => io.total_size(),
        }
    }
}

impl RimRead for ImageIO<'_> {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> rimio::RimIOResult {
        match self {
            Self::Linear(io) => io.read_at(offset, buf),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.read_at(offset, buf),
        }
    }

    fn total_size(&mut self) -> rimio::RimIOResult<u64> {
        match self {
            Self::Linear(io) => io.total_size(),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.total_size(),
        }
    }
}

impl RimWrite for ImageIO<'_> {
    fn write_at(&mut self, offset: u64, data: &[u8]) -> rimio::RimIOResult {
        match self {
            Self::Linear(io) => io.write_at(offset, data),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.write_at(offset, data),
        }
    }

    fn flush(&mut self) -> rimio::RimIOResult {
        match self {
            Self::Linear(io) => io.flush(),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.flush(),
        }
    }
}

impl RimIO for ImageIO<'_> {
    fn set_offset(&mut self, partition_offset: u64) -> u64 {
        match self {
            Self::Linear(io) => io.set_offset(partition_offset),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.set_offset(partition_offset),
        }
    }

    fn partition_offset(&self) -> u64 {
        match self {
            Self::Linear(io) => io.partition_offset(),
            #[cfg(feature = "alloc")]
            Self::Qcow2(io) => io.partition_offset(),
        }
    }
}

#[cfg(feature = "alloc")]
pub fn create_image_io<'a>(
    dst: &'a mut dyn RimIO,
    raw_len: u64,
    format: ImageFormat,
    options: ImageOptions,
) -> RimImgResult<ImageIO<'a>> {
    match format {
        ImageFormat::Raw => Ok(ImageIO::Linear(LinearImageIO::new(
            dst, raw_len, 0, format, options, false,
        ))),
        ImageFormat::Vhd => Ok(ImageIO::Linear(LinearImageIO::new(
            dst, raw_len, 0, format, options, true,
        ))),
        ImageFormat::Vmdk => {
            vmdk::init_vmdk_io(dst, raw_len, options)?;
            Ok(ImageIO::Linear(LinearImageIO::new(
                dst,
                raw_len,
                vmdk::DESCRIPTOR_SECTORS * vmdk::SECTOR_SIZE,
                format,
                options,
                false,
            )))
        }
        ImageFormat::Qcow2 => {
            let qcow2_io = qcow2::create_sparse_qcow2_io(dst, raw_len)?;
            Ok(ImageIO::Qcow2(qcow2_io))
        }
        ImageFormat::Vdi => {
            vdi::init_vdi_io(dst, raw_len, options)?;
            Ok(ImageIO::Linear(LinearImageIO::new(
                dst,
                raw_len,
                vdi::DATA_OFFSET,
                format,
                options,
                false,
            )))
        }
    }
}

pub fn open_image_io(src: &mut dyn RimIO) -> RimImgResult<ImageIO<'_>> {
    let format = ImageFormat::from_io(src)?;
    match format {
        ImageFormat::Raw => {
            let raw_len = src.total_size()?;
            Ok(ImageIO::Linear(LinearImageIO::new(
                src,
                raw_len,
                0,
                format,
                ImageOptions::deterministic(0),
                false,
            )))
        }
        ImageFormat::Vhd => {
            let (raw_len, data_offset) = open_vhd(src)?;
            Ok(ImageIO::Linear(LinearImageIO::new(
                src,
                raw_len,
                data_offset,
                format,
                ImageOptions::deterministic(0),
                false,
            )))
        }
        ImageFormat::Vmdk => {
            let (raw_len, data_offset) = open_vmdk(src)?;
            Ok(ImageIO::Linear(LinearImageIO::new(
                src,
                raw_len,
                data_offset,
                format,
                ImageOptions::deterministic(0),
                false,
            )))
        }
        #[cfg(feature = "alloc")]
        ImageFormat::Qcow2 => {
            let qcow2_io = qcow2::open_sparse_qcow2_io(src)?;
            Ok(ImageIO::Qcow2(qcow2_io))
        }
        #[cfg(not(feature = "alloc"))]
        ImageFormat::Qcow2 => Err(RimImgError::UnsupportedFormat),
        ImageFormat::Vdi => {
            let (raw_len, data_offset) = open_vdi(src)?;
            Ok(ImageIO::Linear(LinearImageIO::new(
                src,
                raw_len,
                data_offset,
                format,
                ImageOptions::deterministic(0),
                false,
            )))
        }
    }
}

pub fn open_image_read_io(src: &mut dyn RimRead) -> RimImgResult<ImageReadIO<'_>> {
    let format = ImageFormat::from_read(src)?;
    match format {
        ImageFormat::Raw => {
            let raw_len = src.total_size()?;
            Ok(ImageReadIO::Linear(LinearImageReadIO::new(
                src, raw_len, 0, format,
            )))
        }
        ImageFormat::Vhd => {
            let (raw_len, data_offset) = open_vhd(src)?;
            Ok(ImageReadIO::Linear(LinearImageReadIO::new(
                src,
                raw_len,
                data_offset,
                format,
            )))
        }
        ImageFormat::Vmdk => {
            let (raw_len, data_offset) = open_vmdk(src)?;
            Ok(ImageReadIO::Linear(LinearImageReadIO::new(
                src,
                raw_len,
                data_offset,
                format,
            )))
        }
        ImageFormat::Qcow2 => {
            let qcow2_reader = qcow2::open_sparse_qcow2_read_io(src)?;
            Ok(ImageReadIO::Qcow2(qcow2_reader))
        }
        ImageFormat::Vdi => {
            let (raw_len, data_offset) = open_vdi(src)?;
            Ok(ImageReadIO::Linear(LinearImageReadIO::new(
                src,
                raw_len,
                data_offset,
                format,
            )))
        }
    }
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

fn open_vdi(src: &mut dyn RimRead) -> RimImgResult<(u64, u64)> {
    let header: vdi::VdiHeader = src.read_struct(64)?;
    vdi::validate_vdi_header(&header)?;
    Ok((header.disk_size.get(), header.offset_data.get() as u64))
}
