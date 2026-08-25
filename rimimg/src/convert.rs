// SPDX-License-Identifier: MIT

use anyhow::Context;
use std::path::Path;

use crate::format::ImageFormat;
use crate::{qcow2, raw, vdi, vhd, vmdk};

/// Wrap a raw image into the requested target container format with progress callback.
pub fn wrap_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    raw_path: P1,
    out_path: P2,
    format: ImageFormat,
    on_progress: F,
) -> anyhow::Result<()> {
    match format {
        ImageFormat::Raw => raw::wrap_raw_as_raw_with_progress(raw_path, out_path, on_progress),
        ImageFormat::Vhd => vhd::wrap_raw_as_vhd_with_progress(raw_path, out_path, on_progress),
        ImageFormat::Vmdk => vmdk::wrap_raw_as_vmdk_with_progress(raw_path, out_path, on_progress),
        ImageFormat::Qcow2 => {
            qcow2::wrap_raw_as_qcow2_with_progress(raw_path, out_path, on_progress)
        }
        ImageFormat::Vdi => vdi::wrap_raw_as_vdi_with_progress(raw_path, out_path, on_progress),
    }
}

/// Wrap a raw image into the requested target container format.
pub fn wrap<P1: AsRef<Path>, P2: AsRef<Path>>(
    raw_path: P1,
    out_path: P2,
    format: ImageFormat,
) -> anyhow::Result<()> {
    wrap_with_progress(raw_path, out_path, format, |_, _| {})
}

/// Unwrap a container image into a raw image file with progress callback.
pub fn unwrap_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    container_path: P1,
    raw_path: P2,
    format: ImageFormat,
    on_progress: F,
) -> anyhow::Result<()> {
    match format {
        ImageFormat::Raw => raw::unwrap_raw_with_progress(container_path, raw_path, on_progress),
        ImageFormat::Vhd => vhd::unwrap_vhd_with_progress(container_path, raw_path, on_progress),
        ImageFormat::Vmdk => vmdk::unwrap_vmdk_with_progress(container_path, raw_path, on_progress),
        ImageFormat::Qcow2 => {
            qcow2::unwrap_qcow2_with_progress(container_path, raw_path, on_progress)
        }
        ImageFormat::Vdi => vdi::unwrap_vdi_with_progress(container_path, raw_path, on_progress),
    }
}

/// Unwrap a container image into a raw image file.
pub fn unwrap<P1: AsRef<Path>, P2: AsRef<Path>>(
    container_path: P1,
    raw_path: P2,
    format: ImageFormat,
) -> anyhow::Result<()> {
    unwrap_with_progress(container_path, raw_path, format, |_, _| {})
}

/// Convert an image file to another format with progress callback.
pub fn convert_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    input_path: P1,
    output_path: P2,
    on_progress: F,
) -> anyhow::Result<()> {
    let in_format = ImageFormat::from_path(input_path.as_ref())
        .with_context(|| format!("Unknown input format for {}", input_path.as_ref().display()))?;
    let out_format = ImageFormat::from_path(output_path.as_ref()).with_context(|| {
        format!(
            "Unknown output format for {}",
            output_path.as_ref().display()
        )
    })?;

    convert_explicit_with_progress(input_path, in_format, output_path, out_format, on_progress)
}

/// Convert an image file to another format.
pub fn convert<P1: AsRef<Path>, P2: AsRef<Path>>(
    input_path: P1,
    output_path: P2,
) -> anyhow::Result<()> {
    convert_with_progress(input_path, output_path, |_, _| {})
}

/// Convert an image with explicitly specified formats and progress callback.
pub fn convert_explicit_with_progress<P1: AsRef<Path>, P2: AsRef<Path>, F: FnMut(u64, u64)>(
    input_path: P1,
    in_format: ImageFormat,
    output_path: P2,
    out_format: ImageFormat,
    mut on_progress: F,
) -> anyhow::Result<()> {
    if in_format == ImageFormat::Raw && out_format == ImageFormat::Raw {
        return raw::wrap_raw_as_raw_with_progress(input_path, output_path, on_progress);
    }

    if in_format == ImageFormat::Raw {
        return wrap_with_progress(input_path, output_path, out_format, on_progress);
    }

    if out_format == ImageFormat::Raw {
        return unwrap_with_progress(input_path, output_path, in_format, on_progress);
    }

    // Container to Container: intermediate raw file
    let temp_root = tempfile::tempdir()?;
    let temp_raw = temp_root.path().join("rim_conversion.img");

    unwrap_with_progress(input_path, &temp_raw, in_format, &mut on_progress)?;
    wrap_with_progress(&temp_raw, output_path, out_format, on_progress)?;

    Ok(())
}

/// Convert an image with explicitly specified formats.
pub fn convert_explicit<P1: AsRef<Path>, P2: AsRef<Path>>(
    input_path: P1,
    in_format: ImageFormat,
    output_path: P2,
    out_format: ImageFormat,
) -> anyhow::Result<()> {
    convert_explicit_with_progress(input_path, in_format, output_path, out_format, |_, _| {})
}
