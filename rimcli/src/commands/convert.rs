// SPDX-License-Identifier: MIT

use crate::ui::{format_duration, pretty_bytes};
use anyhow::Context;
use colored::Colorize;
use rimimg::{ImageFormat, ImageOptions};
use rimio::prelude::FileRimIO;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run(input: PathBuf, output: PathBuf, _verbose: u8, quiet: bool) -> anyhow::Result<()> {
    let t0 = Instant::now();

    if !quiet {
        println!(
            "{}",
            format!("🚀 Rust Image Maker — v{}", env!("CARGO_PKG_VERSION")).bold()
        );
        println!(
            "Converting {} → {}",
            input.display().to_string().cyan(),
            output.display().to_string().cyan()
        );
    }

    let input_len = std::fs::metadata(&input).map(|m| m.len()).unwrap_or(0);

    if !quiet && input_len > 0 {
        let pb = crate::ui::create_byte_progress(input_len, "Streaming");
        convert_file_with_progress(&input, &output, |curr, _total| {
            pb.set_position(curr);
        })?;
        pb.finish_and_clear();
    } else {
        convert_file_with_progress(&input, &output, |_, _| {})?;
    }

    let duration = t0.elapsed();
    let out_bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);

    if !quiet {
        println!(
            "✨ Converted to {} ({} in {}) — with ❤️  from RIM",
            output.display().to_string().bold(),
            pretty_bytes(out_bytes).cyan(),
            format_duration(duration)
        );
    }

    Ok(())
}

pub(crate) fn format_from_path(path: &Path) -> anyhow::Result<ImageFormat> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    ImageFormat::from_extension(ext)
        .with_context(|| format!("Unknown image format for {}", path.display()))
}

pub(crate) fn detect_format_from_file(file: File) -> anyhow::Result<ImageFormat> {
    let mut io = FileRimIO::new(file);
    Ok(ImageFormat::from_io(&mut io)?)
}

pub(crate) fn wrap_file_with_progress<F: FnMut(u64, u64)>(
    input: &Path,
    output: &Path,
    format: ImageFormat,
    mut on_progress: F,
) -> anyhow::Result<()> {
    if format == ImageFormat::Raw && input == output {
        return Ok(());
    }

    let input_file = File::open(input)
        .with_context(|| format!("Failed to open input image {}", input.display()))?;
    let input_len = input_file.metadata()?.len();
    let output_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)
        .with_context(|| format!("Failed to create output image {}", output.display()))?;
    let mut src = FileRimIO::new(input_file);
    let mut dst = FileRimIO::new(output_file);
    let options = ImageOptions::default();

    rimimg::wrap_io_with_progress(
        &mut src,
        &mut dst,
        input_len,
        format,
        options,
        &mut on_progress,
    )?;

    Ok(())
}

pub(crate) fn unwrap_file_with_progress<F: FnMut(u64, u64)>(
    input: &Path,
    output: &Path,
    format: ImageFormat,
    on_progress: F,
) -> anyhow::Result<()> {
    if format == ImageFormat::Raw && input == output {
        return Ok(());
    }

    let input_file = File::open(input)
        .with_context(|| format!("Failed to open input image {}", input.display()))?;
    let input_len = input_file.metadata()?.len();
    let output_file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)
        .with_context(|| format!("Failed to create output image {}", output.display()))?;
    let mut src = FileRimIO::new(input_file);
    let mut dst = FileRimIO::new(output_file);

    rimimg::unwrap_io_with_progress(&mut src, &mut dst, Some(input_len), format, on_progress)?;
    Ok(())
}

pub(crate) fn convert_file_with_progress<F: FnMut(u64, u64)>(
    input: &Path,
    output: &Path,
    mut on_progress: F,
) -> anyhow::Result<()> {
    let input_format = format_from_path(input)?;
    let output_format = format_from_path(output)?;

    if input_format == ImageFormat::Raw {
        return wrap_file_with_progress(input, output, output_format, on_progress);
    }

    if output_format == ImageFormat::Raw {
        return unwrap_file_with_progress(input, output, input_format, on_progress);
    }

    let temp = tempfile::NamedTempFile::new()
        .context("Failed to create temp file for container conversion")?;
    unwrap_file_with_progress(input, temp.path(), input_format, &mut on_progress)?;
    wrap_file_with_progress(temp.path(), output, output_format, on_progress)
}
