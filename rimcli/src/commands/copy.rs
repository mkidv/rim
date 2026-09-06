// SPDX-License-Identifier: MIT

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::Context;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

use rimfs::core::resolver::FsTreeResolver;
use rimfs::core::{StdInjector, StdOverwritePolicy, StdResolver};
use rimfs::{exfat, ext, fat, iso, ntfs, tar, zip};
use rimimg::ImageFormat;
use rimio::RimIO;
use rimio::prelude::{OverlayRimIO, StdRimIO};
use tempfile::NamedTempFile;

use crate::commands::convert::{format_from_path, unwrap_file_with_progress};
use crate::copy::dry_run::DryRunStdInjector;
use crate::copy::engine::copy_tree;
use crate::copy::options::{
    CopyOptions, MetadataPolicy, OverwritePolicy, UnsupportedMetadataPolicy,
};
use crate::copy::progress::CopyEvent;
use crate::copy::report::CopyReport;
use crate::ui::format::pretty_bytes;

#[derive(Debug, Clone, Copy)]
pub struct SparseStats {
    pub allocated_bytes: u64,
    pub allocated_pages: usize,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    source: String,
    destination: String,
    src_path: String,
    metadata: String,
    unsupported: String,
    overwrite: Option<String>,
    dry_run: bool,
    no_detect_case_collisions: bool,
    verbose: u8,
    quiet: bool,
) -> anyhow::Result<()> {
    let dst_p = Path::new(&destination);
    let default_overwrite = if !dst_p.exists() || dst_p.is_dir() {
        OverwritePolicy::Replace
    } else {
        OverwritePolicy::Error
    };

    let overwrite_policy = match overwrite.as_deref() {
        Some(s) => OverwritePolicy::from_str(s).map_err(|e| anyhow::anyhow!("{e}"))?,
        None => default_overwrite,
    };

    let options = CopyOptions {
        metadata_policy: MetadataPolicy::from_str(&metadata).map_err(|e| anyhow::anyhow!("{e}"))?,
        unsupported_policy: UnsupportedMetadataPolicy::from_str(&unsupported)
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        overwrite_policy,
        buffer_size: 64 * 1024,
        detect_case_collisions: !no_detect_case_collisions,
        destination_case_sensitive: true,
        destination_supports_replace: false,
        destination_supports_symlinks: true,
        dry_run,
    };

    if !quiet {
        println!(
            "{}",
            format!("📋 Logical Filesystem Copy: {source} ➔ {destination}")
                .bold()
                .cyan()
        );
        if options.dry_run {
            println!(
                "  {}",
                "🔎 DRY-RUN MODE: Simulation only, no data will be written to destination"
                    .yellow()
                    .bold()
            );
        }
        if verbose > 0 {
            println!("  Metadata Policy    : {:?}", options.metadata_policy);
            println!("  Unsupported Policy : {:?}", options.unsupported_policy);
            println!("  Overwrite Policy   : {:?}", options.overwrite_policy);
            println!("  Case Collisions    : {}", options.detect_case_collisions);
            println!("  Dry-run            : {}", options.dry_run);
        }
    }

    let pb = if !quiet {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} [{elapsed_precise}] {msg}")
                .unwrap(),
        );
        bar.enable_steady_tick(std::time::Duration::from_millis(100));
        Some(bar)
    } else {
        None
    };

    let mut progress_callback = |event: CopyEvent<'_>| match event {
        CopyEvent::StartingFile { path, size } => {
            if let Some(bar) = &pb {
                bar.set_message(format!("Copying {path} ({})", pretty_bytes(size)));
            }
        }
        CopyEvent::StartingDirectory { path } => {
            if let Some(bar) = &pb {
                bar.set_message(format!("Entering directory {path}"));
            }
        }
        CopyEvent::Warning { warning } if !quiet => {
            eprintln!(
                "{} {}",
                "⚠️ [WARN]".yellow().bold(),
                warning.message.yellow()
            );
        }
        _ => {}
    };

    let src_path_ref = Path::new(&source);

    // Check if source is a host directory
    if src_path_ref.is_dir() {
        let mut resolver = StdResolver::new();
        let effective_src = if src_path == "/" {
            format!("{source}/*")
        } else {
            src_path
        };
        let (report, stats) = dispatch_destination(
            &destination,
            &mut resolver,
            &effective_src,
            &options,
            Some(&mut progress_callback),
        )?;
        finish_report(report, quiet, pb, options.dry_run, stats);
        return Ok(());
    }

    // Source is a file: could be host single file, or image/archive
    let (unwrapped_src, src_file_path) = prepare_image_file(src_path_ref)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&src_file_path)
        .with_context(|| format!("Failed to open source file: {}", src_file_path.display()))?;

    let file_len = file.metadata()?.len();
    let mut io = StdRimIO::new(&mut file);

    // Detect if source has partitions
    if let Ok(scan) = rimpart::scan_disk_with_sector(&mut io, 512)
        && let Some(p) = scan.partitions.first()
    {
        io.set_offset(p.start_lba * 512);
    }

    let (report, stats) = if let Ok(meta) = ext::ExtMeta::from_io(&mut io) {
        let mut resolver = ext::ExtResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else if let Ok(meta) = ntfs::NtfsMeta::from_io(&mut io) {
        let mut resolver = ntfs::NtfsResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else if let Ok(meta) = exfat::ExFatMeta::from_io(&mut io) {
        let mut resolver = exfat::ExFatResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else if let Ok(meta) = fat::FatMeta::from_io(&mut io) {
        let mut resolver = fat::FatResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else if source.ends_with(".tar") || tar::TarMeta::new(file_len, None).is_ok() {
        let meta = tar::TarMeta::new(file_len, None)?;
        let mut resolver = tar::TarResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else if source.ends_with(".zip") || zip::ZipMeta::new(file_len, None).is_ok() {
        let meta = zip::ZipMeta::new(file_len, None)?;
        let mut resolver = zip::ZipResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else if source.ends_with(".iso") || iso::IsoMeta::new(file_len, None).is_ok() {
        let meta = iso::IsoMeta::new(file_len, None)?;
        let mut resolver = iso::IsoResolver::new(&mut io, &meta);
        dispatch_destination(
            &destination,
            &mut resolver,
            &src_path,
            &options,
            Some(&mut progress_callback),
        )?
    } else {
        // Fallback: single regular host file
        let mut resolver = StdResolver::new();
        let (report, stats) = dispatch_destination(
            &destination,
            &mut resolver,
            &source,
            &options,
            Some(&mut progress_callback),
        )?;
        finish_report(report, quiet, pb, options.dry_run, stats);
        return Ok(());
    };

    drop(unwrapped_src);
    finish_report(report, quiet, pb, options.dry_run, stats);
    Ok(())
}

fn prepare_image_file(path: &Path) -> anyhow::Result<(Option<NamedTempFile>, PathBuf)> {
    let format = format_from_path(path).unwrap_or(ImageFormat::Raw);
    if format == ImageFormat::Raw {
        Ok((None, path.to_path_buf()))
    } else {
        let tmp = NamedTempFile::new().context("Failed to create temp file for image unwrap")?;
        let tmp_path = tmp.path().to_path_buf();
        unwrap_file_with_progress(path, &tmp_path, format, |_, _| {})?;
        Ok((Some(tmp), tmp_path))
    }
}

fn dispatch_destination(
    destination: &str,
    resolver: &mut dyn FsTreeResolver,
    src_path: &str,
    options: &CopyOptions,
    progress: Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> anyhow::Result<(CopyReport, Option<SparseStats>)> {
    let dst_path_ref = Path::new(destination);

    // If destination does not exist or is a directory: use Host injector
    if !dst_path_ref.exists() || dst_path_ref.is_dir() {
        let std_policy = match options.overwrite_policy {
            OverwritePolicy::Replace => StdOverwritePolicy::Replace,
            OverwritePolicy::Error => StdOverwritePolicy::Error,
            OverwritePolicy::Skip => StdOverwritePolicy::Skip,
        };

        let mut opts = options.clone();
        opts.destination_case_sensitive = cfg!(unix) && !cfg!(target_os = "macos");
        opts.destination_supports_replace = true;
        opts.destination_supports_symlinks = true;

        let report = if options.dry_run {
            let mut injector =
                DryRunStdInjector::new(dst_path_ref).with_overwrite_policy(std_policy);
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))?
        } else {
            let mut injector = StdInjector::new(dst_path_ref)
                .with_context(|| format!("Failed to initialize StdInjector at '{destination}'"))?
                .with_overwrite_policy(std_policy);
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))?
        };

        return Ok((report, None));
    }

    // Destination is an existing file: detect filesystem image format
    if options.overwrite_policy == OverwritePolicy::Replace {
        anyhow::bail!(
            "In-place entry replacement ('--overwrite replace') is unsupported on filesystem images; supported only on Host destinations"
        );
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(!options.dry_run)
        .open(dst_path_ref)
        .with_context(|| format!("Failed to open destination image '{destination}'"))?;

    let file_len = file.metadata()?.len();
    let mut std_io = StdRimIO::new(&mut file);

    if options.dry_run {
        let mut cow_io = OverlayRimIO::new(&mut std_io, file_len);
        let report = dispatch_image_fs(
            destination,
            file_len,
            &mut cow_io,
            resolver,
            src_path,
            options,
            progress,
        )?;
        let stats = Some(SparseStats {
            allocated_bytes: cow_io.allocated_bytes(),
            allocated_pages: cow_io.allocated_pages(),
        });
        Ok((report, stats))
    } else {
        let report = dispatch_image_fs(
            destination,
            file_len,
            &mut std_io,
            resolver,
            src_path,
            options,
            progress,
        )?;
        Ok((report, None))
    }
}

fn dispatch_image_fs(
    destination: &str,
    file_len: u64,
    io: &mut dyn RimIO,
    resolver: &mut dyn FsTreeResolver,
    src_path: &str,
    options: &CopyOptions,
    progress: Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> anyhow::Result<CopyReport> {
    if let Ok(scan) = rimpart::scan_disk_with_sector(io, 512)
        && let Some(p) = scan.partitions.first()
    {
        io.set_offset(p.start_lba * 512);
    }

    if let Ok(meta) = ext::ExtMeta::from_io(io) {
        let mut injector = ext::ExtInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = true;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = true;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else if let Ok(meta) = ntfs::NtfsMeta::from_io(io) {
        let mut injector = ntfs::NtfsInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = false;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else if let Ok(meta) = exfat::ExFatMeta::from_io(io) {
        let mut injector = exfat::ExFatInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = false;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else if let Ok(meta) = fat::FatMeta::from_io(io) {
        let mut injector = fat::FatInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = false;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else if destination.ends_with(".tar") || tar::TarMeta::new(file_len, None).is_ok() {
        let meta = tar::TarMeta::new(file_len, None)?;
        let mut injector = tar::TarInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = true;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = true;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else if destination.ends_with(".zip") || zip::ZipMeta::new(file_len, None).is_ok() {
        let meta = zip::ZipMeta::new(file_len, None)?;
        let mut injector = zip::ZipInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = true;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = true;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else if destination.ends_with(".iso") || iso::IsoMeta::new(file_len, None).is_ok() {
        let meta = iso::IsoMeta::new(file_len, None)?;
        let mut injector = iso::IsoInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        opts.destination_supports_symlinks = false;
        copy_tree(resolver, &mut injector, src_path, &opts, progress)
            .map_err(|e| anyhow::anyhow!("{e}"))
    } else {
        anyhow::bail!(
            "Destination file '{}' is not a recognized or supported filesystem image",
            destination
        );
    }
}

fn finish_report(
    report: CopyReport,
    quiet: bool,
    pb: Option<ProgressBar>,
    is_dry_run: bool,
    stats: Option<SparseStats>,
) {
    if let Some(bar) = pb {
        bar.finish_and_clear();
    }

    if !quiet {
        if is_dry_run {
            println!(
                "{}",
                "✔ Dry-run completed successfully! (Simulation only, no data written)"
                    .yellow()
                    .bold()
            );
            if let Some(s) = stats {
                println!(
                    "  {}",
                    format!(
                        "🌀 Sparse simulation: {} allocated in memory • {} pages modified",
                        pretty_bytes(s.allocated_bytes),
                        s.allocated_pages
                    )
                    .cyan()
                );
            }
        } else {
            println!("{}", "✔ Copy completed successfully!".green().bold());
        }
        println!("  Directories Created : {}", report.directories_created);
        println!("  Files Copied        : {}", report.files_copied);
        println!("  Symlinks Created    : {}", report.symlinks_created);
        println!(
            "  Data Transferred    : {} ({} bytes)",
            pretty_bytes(report.bytes_transferred).cyan(),
            report.bytes_transferred
        );
        println!("  Duration            : {:.2?}", report.duration);

        if !report.warnings.is_empty() {
            println!(
                "  Warnings            : {}",
                report.warnings.len().to_string().yellow().bold()
            );
        }
    }
}
