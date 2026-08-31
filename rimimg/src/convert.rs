// SPDX-License-Identifier: MIT

use rimio::prelude::*;

#[cfg(feature = "alloc")]
use crate::ImageOptions;
use crate::errors::{RimImgError, RimImgResult};
use crate::format::ImageFormat;
use crate::{qcow2, raw, vdi, vhd, vmdk};

#[cfg(feature = "alloc")]
pub fn wrap_io(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    raw_len: u64,
    format: ImageFormat,
    options: ImageOptions,
) -> RimImgResult {
    wrap_io_with_progress(src, dst, raw_len, format, options, |_, _| {})
}

#[cfg(feature = "alloc")]
pub fn wrap_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimIO,
    raw_len: u64,
    format: ImageFormat,
    options: ImageOptions,
    on_progress: F,
) -> RimImgResult {
    match format {
        ImageFormat::Raw => raw::wrap_raw_as_raw_io_with_progress(src, dst, raw_len, on_progress),
        ImageFormat::Vhd => {
            vhd::wrap_raw_as_vhd_io_with_progress(src, dst, raw_len, options, on_progress)
        }
        ImageFormat::Vmdk => {
            vmdk::wrap_raw_as_vmdk_io_with_progress(src, dst, raw_len, options, on_progress)
        }
        ImageFormat::Qcow2 => {
            qcow2::wrap_raw_as_qcow2_io_with_progress(src, dst, raw_len, on_progress)
        }
        ImageFormat::Vdi => {
            vdi::wrap_raw_as_vdi_io_with_progress(src, dst, raw_len, options, on_progress)
        }
    }
}

pub fn unwrap_io(
    src: &mut dyn RimIO,
    dst: &mut dyn RimWrite,
    raw_len: Option<u64>,
    format: ImageFormat,
) -> RimImgResult {
    unwrap_io_with_progress(src, dst, raw_len, format, |_, _| {})
}

pub fn unwrap_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimIO,
    dst: &mut dyn RimWrite,
    raw_len: Option<u64>,
    format: ImageFormat,
    on_progress: F,
) -> RimImgResult {
    match format {
        ImageFormat::Raw => {
            let len = raw_len.ok_or(RimImgError::InvalidHeader("RAW length is required"))?;
            raw::unwrap_raw_io_with_progress(src, dst, len, on_progress)
        }
        ImageFormat::Vhd => vhd::unwrap_vhd_io_with_progress(src, dst, on_progress),
        ImageFormat::Vmdk => vmdk::unwrap_vmdk_io_with_progress(src, dst, on_progress),
        ImageFormat::Qcow2 => qcow2::unwrap_qcow2_io_with_progress(src, dst, on_progress),
        ImageFormat::Vdi => vdi::unwrap_vdi_io_with_progress(src, dst, on_progress),
    }
}
