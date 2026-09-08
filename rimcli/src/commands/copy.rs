// SPDX-License-Identifier: MIT

use std::fs::OpenOptions;
use std::path::Path;
use std::str::FromStr;

use anyhow::Context;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

use rimfs::core::resolver::FsTreeResolver;
use rimfs::core::{StdInjector, StdOverwritePolicy, StdResolver};
use rimfs::{exfat, ext, fat, iso, ntfs, tar, zip};
use rimio::RimIO;
use rimio::prelude::{OverlayRimIO, StdRimIO};

use crate::copy::dry_run::DryRunStdInjector;
use crate::copy::endpoint::CopyEndpoint;
use crate::copy::engine::copy_tree;
use crate::copy::options::{
    CopyOptions, MetadataPolicy, OverwritePolicy, UnsupportedMetadataPolicy,
};
use crate::copy::progress::CopyEvent;
use crate::copy::report::CopyReport;
use crate::copy::subpath::SubpathInjector;
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
    let src_endpoint = CopyEndpoint::parse(&source)?;
    let dst_endpoint = CopyEndpoint::parse(&destination)?;

    // Define precedence between endpoint internal path and legacy `--src-path`
    let effective_src = if src_endpoint.internal_path != "/" && src_path != "/" {
        if src_endpoint.internal_path != src_path {
            anyhow::bail!(
                "Conflicting source paths specified: endpoint internal path is '{}' but '--src-path' is '{}'. Specify the subpath either in the endpoint syntax or via '--src-path', not both with different values.",
                src_endpoint.internal_path,
                src_path
            );
        }
        src_endpoint.internal_path.clone()
    } else if src_endpoint.internal_path != "/" {
        src_endpoint.internal_path.clone()
    } else {
        src_path
    };

    let default_overwrite = if !dst_endpoint.is_explicit_image
        && (!dst_endpoint.host_path.exists() || dst_endpoint.host_path.is_dir())
    {
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
        detect_case_collisions: !no_detect_case_collisions,
        destination_case_sensitive: true,
        destination_supports_replace: false,
    };

    if !quiet {
        println!(
            "{}",
            format!("📋 Logical Filesystem Copy: {source} ➔ {destination}")
                .bold()
                .cyan()
        );
        if dry_run {
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
            println!("  Dry-run            : {}", dry_run);
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

    // Check if source is a host directory
    if src_endpoint.host_path.is_dir() {
        if src_endpoint.partition.is_some() {
            anyhow::bail!(
                "Partition selector cannot be used on a host directory: {}",
                src_endpoint.host_path.display()
            );
        }
        let mut resolver = StdResolver::new();
        let src_str = src_endpoint.host_path.to_string_lossy().to_string();
        let resolved_src = if effective_src == "/" {
            format!("{src_str}/*")
        } else if effective_src.starts_with('/') {
            format!("{src_str}{effective_src}")
        } else {
            format!("{src_str}/{effective_src}")
        };
        let (report, stats) = dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &resolved_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?;
        finish_report(report, quiet, pb, dry_run, stats);
        return Ok(());
    }

    // Source is a file: open read-only
    let mut file = OpenOptions::new()
        .read(true)
        .write(false)
        .open(&src_endpoint.host_path)
        .with_context(|| {
            format!(
                "Failed to open source file: {}",
                src_endpoint.host_path.display()
            )
        })?;

    let mut std_io = StdRimIO::new(&mut file);
    let mut image_io = rimimg::open_image_io(&mut std_io).with_context(|| {
        format!(
            "Failed to open source image container: {}",
            src_endpoint.host_path.display()
        )
    })?;

    // Partition selection contract
    resolve_partition(
        &mut image_io,
        &src_endpoint.host_path,
        src_endpoint.partition,
    )?;

    let raw_len = image_io.raw_len();

    let (report, stats) = if let Ok(meta) = ext::ExtMeta::from_io(&mut image_io) {
        let mut resolver = ext::ExtResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if let Ok(meta) = ntfs::NtfsMeta::from_io(&mut image_io) {
        let mut resolver = ntfs::NtfsResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if let Ok(meta) = exfat::ExFatMeta::from_io(&mut image_io) {
        let mut resolver = exfat::ExFatResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if let Ok(meta) = fat::FatMeta::from_io(&mut image_io) {
        let mut resolver = fat::FatResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if src_endpoint
        .host_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tar"))
    {
        let meta = tar::TarMeta::new(raw_len, None)?;
        let mut resolver = tar::TarResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if src_endpoint
        .host_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        let meta = zip::ZipMeta::new(raw_len, None)?;
        let mut resolver = zip::ZipResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if src_endpoint
        .host_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
    {
        let meta = iso::IsoMeta::new(raw_len, None)?;
        let mut resolver = iso::IsoResolver::new(&mut image_io, &meta);
        dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &effective_src,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?
    } else if src_endpoint.is_explicit_image {
        anyhow::bail!(
            "Source '{}' was specified as an image or partition, but no recognized filesystem or archive was found",
            src_endpoint.host_path.display()
        );
    } else {
        // Fallback: single regular host file
        let mut resolver = StdResolver::new();
        let src_file_str = src_endpoint.host_path.to_string_lossy().to_string();
        let (report, stats) = dispatch_destination(
            &dst_endpoint,
            &mut resolver,
            &src_file_str,
            &options,
            dry_run,
            Some(&mut progress_callback),
        )?;
        finish_report(report, quiet, pb, dry_run, stats);
        return Ok(());
    };

    finish_report(report, quiet, pb, dry_run, stats);
    Ok(())
}

fn resolve_partition(
    io: &mut dyn RimIO,
    path: &Path,
    partition_selector: Option<usize>,
) -> anyhow::Result<()> {
    match rimpart::scan_disk_with_sector(io, 512) {
        Ok(scan) if !scan.partitions.is_empty() => {
            if let Some(idx) = partition_selector {
                if idx == 0 || idx > scan.partitions.len() {
                    let max_p = scan.partitions.len();
                    anyhow::bail!(
                        "Partition {idx} does not exist in image '{}' (available partitions: 1..={max_p})",
                        path.display()
                    );
                }
                let part = &scan.partitions[idx - 1];
                io.set_offset(part.start_lba * scan.sector_size);
                Ok(())
            } else if scan.partitions.len() == 1 {
                // 1 partition + no selector -> automatically select partition 1
                let part = &scan.partitions[0];
                io.set_offset(part.start_lba * scan.sector_size);
                Ok(())
            } else {
                // 2+ partitions + no selector -> fail deterministically and list available partitions
                let mut parts_desc = Vec::new();
                for (i, p) in scan.partitions.iter().enumerate() {
                    let num = i + 1;
                    let name = if p.name.is_empty() {
                        "unnamed"
                    } else {
                        &p.name
                    };
                    parts_desc.push(format!(
                        "  [{num}] {name} (type: {}, size: {} bytes)",
                        p.kind, p.size_bytes
                    ));
                }
                anyhow::bail!(
                    "Image '{}' contains {} partitions, but no partition was specified.\nSpecify a partition with '<image>:<partition_number>:<path>' (e.g. {}:1:/).\nAvailable partitions:\n{}",
                    path.display(),
                    scan.partitions.len(),
                    path.display(),
                    parts_desc.join("\n")
                );
            }
        }
        Ok(_) => {
            // 0 partitions -> treat as raw/unpartitioned filesystem
            if let Some(idx) = partition_selector {
                anyhow::bail!(
                    "Partition {idx} was specified, but image '{}' contains no partition table (image is unpartitioned)",
                    path.display()
                );
            }
            Ok(())
        }
        Err(_) => {
            // Not a partitioned disk (e.g. bare filesystem or archive)
            if let Some(idx) = partition_selector {
                anyhow::bail!(
                    "Partition {idx} was specified, but image '{}' does not have a valid partition table",
                    path.display()
                );
            }
            Ok(())
        }
    }
}

fn dispatch_destination(
    dst_endpoint: &CopyEndpoint,
    resolver: &mut dyn FsTreeResolver,
    src_path: &str,
    options: &CopyOptions,
    is_dry_run: bool,
    progress: Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> anyhow::Result<(CopyReport, Option<SparseStats>)> {
    // If destination does not exist or is a directory (and not an explicit image): use Host injector
    if !dst_endpoint.is_explicit_image
        && (!dst_endpoint.host_path.exists() || dst_endpoint.host_path.is_dir())
    {
        if dst_endpoint.partition.is_some() {
            anyhow::bail!(
                "Partition selector cannot be used with host filesystem destination '{}'",
                dst_endpoint.host_path.display()
            );
        }

        let target_dir = if dst_endpoint.internal_path == "/" {
            dst_endpoint.host_path.clone()
        } else {
            dst_endpoint
                .host_path
                .join(dst_endpoint.internal_path.trim_start_matches('/'))
        };

        let std_policy = match options.overwrite_policy {
            OverwritePolicy::Replace => StdOverwritePolicy::Replace,
            OverwritePolicy::Error => StdOverwritePolicy::Error,
            OverwritePolicy::Skip => StdOverwritePolicy::Skip,
        };

        let mut opts = options.clone();
        opts.destination_case_sensitive = cfg!(unix) && !cfg!(target_os = "macos");
        opts.destination_supports_replace = true;

        let report = if is_dry_run {
            let mut injector =
                DryRunStdInjector::new(&target_dir).with_overwrite_policy(std_policy);
            let mut report = copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let skipped = injector.skipped_count();
            report.files_copied = report.files_copied.saturating_sub(skipped);
            report.files_skipped += skipped;
            report
        } else {
            let mut injector = StdInjector::new(&target_dir)
                .with_context(|| {
                    format!(
                        "Failed to initialize StdInjector at '{}'",
                        target_dir.display()
                    )
                })?
                .with_overwrite_policy(std_policy);
            let mut report = copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let skipped = injector.skipped_count();
            report.files_copied = report.files_copied.saturating_sub(skipped);
            report.files_skipped += skipped;
            report
        };

        return Ok((report, None));
    }

    // Destination is an existing file or explicit image
    if options.overwrite_policy == OverwritePolicy::Replace {
        anyhow::bail!(
            "In-place entry replacement ('--overwrite replace') is unsupported on filesystem images; supported only on Host destinations"
        );
    }

    if !dst_endpoint.host_path.exists() {
        anyhow::bail!(
            "Destination image file '{}' does not exist",
            dst_endpoint.host_path.display()
        );
    }

    let mut file = OpenOptions::new()
        .read(true)
        .write(!is_dry_run)
        .open(&dst_endpoint.host_path)
        .with_context(|| {
            format!(
                "Failed to open destination image '{}'",
                dst_endpoint.host_path.display()
            )
        })?;

    let mut std_io = StdRimIO::new(&mut file);
    let mut image_io = rimimg::open_image_io(&mut std_io)?;
    let raw_len = image_io.raw_len();

    if is_dry_run {
        let mut cow_io = OverlayRimIO::new(&mut image_io, raw_len);
        resolve_partition(&mut cow_io, &dst_endpoint.host_path, dst_endpoint.partition)?;
        let report = dispatch_image_fs(
            dst_endpoint,
            raw_len,
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
        resolve_partition(
            &mut image_io,
            &dst_endpoint.host_path,
            dst_endpoint.partition,
        )?;
        let report = dispatch_image_fs(
            dst_endpoint,
            raw_len,
            &mut image_io,
            resolver,
            src_path,
            options,
            progress,
        )?;
        Ok((report, None))
    }
}

fn dispatch_image_fs(
    dst_endpoint: &CopyEndpoint,
    raw_len: u64,
    io: &mut dyn RimIO,
    resolver: &mut dyn FsTreeResolver,
    src_path: &str,
    options: &CopyOptions,
    progress: Option<&mut dyn FnMut(CopyEvent<'_>)>,
) -> anyhow::Result<CopyReport> {
    let subpath = &dst_endpoint.internal_path;

    if let Ok(meta) = ext::ExtMeta::from_io(io) {
        let mut injector = ext::ExtInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = true;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else if let Ok(meta) = ntfs::NtfsMeta::from_io(io) {
        let mut injector = ntfs::NtfsInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else if let Ok(meta) = exfat::ExFatMeta::from_io(io) {
        let mut injector = exfat::ExFatInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else if let Ok(meta) = fat::FatMeta::from_io(io) {
        let mut injector = fat::FatInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else if dst_endpoint
        .host_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tar"))
        || tar::TarMeta::new(raw_len, None).is_ok()
    {
        let meta = tar::TarMeta::new(raw_len, None)?;
        let mut injector = tar::TarInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = true;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else if dst_endpoint
        .host_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        || zip::ZipMeta::new(raw_len, None).is_ok()
    {
        let meta = zip::ZipMeta::new(raw_len, None)?;
        let mut injector = zip::ZipInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = true;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else if dst_endpoint
        .host_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("iso"))
        || iso::IsoMeta::new(raw_len, None).is_ok()
    {
        let meta = iso::IsoMeta::new(raw_len, None)?;
        let mut injector = iso::IsoInjector::new(io, &meta)?;
        let mut opts = options.clone();
        opts.destination_case_sensitive = false;
        opts.destination_supports_replace = false;
        if subpath != "/" {
            let mut sub_injector = SubpathInjector::new(&mut injector, subpath);
            copy_tree(resolver, &mut sub_injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        } else {
            copy_tree(resolver, &mut injector, src_path, &opts, progress)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    } else {
        anyhow::bail!(
            "Destination file '{}' is not a recognized or supported filesystem image",
            dst_endpoint.host_path.display()
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
        if report.files_skipped > 0 {
            println!("  Files Skipped       : {}", report.files_skipped);
        }
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
