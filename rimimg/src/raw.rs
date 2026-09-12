// SPDX-License-Identifier: MIT

//! Raw unencapsulated disk image format support.

use rimio::prelude::*;

use crate::errors::RimImgResult;

pub fn wrap_raw_as_raw_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimWrite,
    len: u64,
    on_progress: F,
) -> RimImgResult {
    dst.copy_from_with_progress(src, 0, 0, len, on_progress)?;
    dst.flush()?;
    Ok(())
}

pub fn wrap_raw_as_raw_io(src: &mut dyn RimRead, dst: &mut dyn RimWrite, len: u64) -> RimImgResult {
    wrap_raw_as_raw_io_with_progress(src, dst, len, |_, _| {})
}

pub fn unwrap_raw_io_with_progress<F: FnMut(u64, u64)>(
    src: &mut dyn RimRead,
    dst: &mut dyn RimWrite,
    len: u64,
    on_progress: F,
) -> RimImgResult {
    wrap_raw_as_raw_io_with_progress(src, dst, len, on_progress)
}

pub fn unwrap_raw_io(src: &mut dyn RimRead, dst: &mut dyn RimWrite, len: u64) -> RimImgResult {
    unwrap_raw_io_with_progress(src, dst, len, |_, _| {})
}
