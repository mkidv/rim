// SPDX-License-Identifier: MIT

use anyhow::Context;
use rimio::prelude::*;
use std::fs::File;
use std::path::Path;

/// Wrap raw .img as RAW with progress callback.
pub fn wrap_raw_as_raw_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    img_path: P1,
    raw_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    if img_path.as_ref() == raw_path.as_ref() {
        return Ok(());
    }
    let mut in_file = File::open(img_path.as_ref())
        .with_context(|| format!("Failed to open input image {}", img_path.as_ref().display()))?;
    let len = in_file.metadata()?.len();
    let mut in_io = StdRimIO::new(&mut in_file);

    let mut out_file = File::create(raw_path.as_ref())
        .with_context(|| format!("Failed to create raw image {}", raw_path.as_ref().display()))?;
    let mut out_io = StdRimIO::new(&mut out_file);

    out_io.copy_from_with_progress(&mut in_io, 0, 0, len, on_progress)?;
    out_io.flush()?;
    Ok(())
}

pub fn wrap_raw_as_raw_to<P1: AsRef<Path>, P2: AsRef<Path>>(
    img_path: P1,
    raw_path: P2,
) -> anyhow::Result<()> {
    wrap_raw_as_raw_with_progress(img_path, raw_path, |_, _| {})
}

/// Strip RAW and restore .img with progress callback.
pub fn unwrap_raw_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    raw_path: P1,
    img_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    wrap_raw_as_raw_with_progress(raw_path, img_path, on_progress)
}

pub fn unwrap_raw_to_raw<P1: AsRef<Path>, P2: AsRef<Path>>(
    raw_path: P1,
    img_path: P2,
) -> anyhow::Result<()> {
    unwrap_raw_with_progress(raw_path, img_path, |_, _| {})
}
