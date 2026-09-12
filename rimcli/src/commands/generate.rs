// SPDX-License-Identifier: MIT

//! CLI command to build disk images from declarative layout specifications.

use crate::ui::badge::fs_badge;
use crate::ui::format::{format_duration, pretty_bytes, sep_u64};
use crate::ui::progress::create_spinner;
use crate::ui::table::print_layout_table;
use anyhow::anyhow;
use colored::Colorize;
use rimgen::{BuildEvent, BuildOptions, LayoutConfig, PartitionTable};
use rimimg::ImageFormat;
use rimio::prelude::*;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[allow(clippy::too_many_arguments)]
pub fn run(
    layout_path: PathBuf,
    output: Option<PathBuf>,
    _truncate: bool,
    dry_run: bool,
    host: bool,
    no_gpt: bool,
    verbose: u8,
    quiet: bool,
) -> anyhow::Result<()> {
    let t0 = Instant::now();

    if !quiet {
        println!(
            "{}",
            format!("🚀 Rust Image Maker — v{}", env!("CARGO_PKG_VERSION")).bold()
        );
    }

    let layout = LayoutConfig::from_file(&layout_path)?;
    layout.validate()?;

    let out_path = output.unwrap_or_else(|| layout_path.with_extension("img"));
    let build_options = BuildOptions {
        partition_table: if no_gpt {
            PartitionTable::None
        } else {
            layout.effective_partition_table()
        },
    };

    if !quiet {
        if dry_run {
            println!("🌀 Dry run mode: no permanent data will be written.");
        } else {
            println!(
                "Writing disk image to {}",
                out_path.display().to_string().cyan()
            );
        }

        if verbose > 0 {
            println!("\n📋 Planned layout:");
            print_layout_table(&layout);
        }

        if build_options.partition_table == PartitionTable::None {
            println!("Generating without GPT or protective MBR.");
        }
    }

    if host {
        if build_options.partition_table == PartitionTable::None {
            return Err(anyhow!(
                "--no-gpt or table = 'none' cannot be combined with --host"
            ));
        }

        if !quiet {
            println!("🛠️  Using OS-native host integration (rimhost)...");
        }
        rimhost::format_inject_host(&layout, &out_path, dry_run)?;
    } else {
        let spinner = if !quiet {
            Some(create_spinner("Initializing disk builder..."))
        } else {
            None
        };

        let spinner_ref = spinner.clone();

        let mut on_event = move |event: BuildEvent<'_>| {
            if let Some(sp) = &spinner_ref {
                match event {
                    BuildEvent::LayoutPlanned {
                        total_bytes,
                        total_sectors,
                    } => {
                        sp.set_message(format!(
                            "Planning layout: {} ({} sectors)",
                            pretty_bytes(total_bytes).cyan(),
                            sep_u64(total_sectors)
                        ));
                    }
                    BuildEvent::GptWritten { duration } => {
                        sp.println(format!(
                            "{} Protective MBR & GPT partition table written in {}",
                            "✔".green().bold(),
                            format_duration(duration).cyan()
                        ));
                    }
                    BuildEvent::PartitionStart { index, total, name } => {
                        sp.set_message(format!(
                            "Processing partition [{}/{}] '{}'...",
                            index + 1,
                            total,
                            name.bold()
                        ));
                    }
                    BuildEvent::PartitionFormatted(rep) => {
                        let mut parts = Vec::new();
                        if rep.dirs_count > 0 {
                            parts.push(format!("{} dirs", rep.dirs_count));
                        }
                        if rep.files_count > 0 {
                            parts.push(format!("{} files", rep.files_count));
                        }
                        if rep.symlinks_count > 0 {
                            parts.push(format!("{} symlinks", rep.symlinks_count));
                        }
                        let content_str = if !parts.is_empty() {
                            format!(" • {} injected", parts.join(" • "))
                        } else {
                            String::new()
                        };
                        sp.println(format!(
                            "{} \"{}\" formatted in {}{} in {}",
                            "✔".green().bold(),
                            rep.name.bold(),
                            fs_badge(&rep.fs),
                            content_str,
                            format_duration(rep.duration).cyan()
                        ));
                    }
                    BuildEvent::PayloadProgress {
                        current_bytes,
                        total_bytes,
                    } => {
                        sp.set_message(format!(
                            "Writing raw payload: {} / {}",
                            pretty_bytes(current_bytes).cyan(),
                            pretty_bytes(total_bytes).cyan()
                        ));
                    }
                }
            }
        };

        let dry_stats = if dry_run {
            Some(build_config_dry_run(&layout, build_options, &mut on_event)?)
        } else {
            build_config_to_file(&layout, &out_path, build_options, &mut on_event)?;
            None
        };

        if let Some(sp) = spinner {
            sp.finish_and_clear();
        }

        if verbose > 0
            && let Some(stats) = dry_stats
        {
            println!(
                "🌀 Sparse simulation: {} logical • {} allocated • {} pages",
                pretty_bytes(stats.logical_bytes),
                pretty_bytes(stats.allocated_bytes),
                stats.allocated_pages,
            );
        }
    }

    let dt = t0.elapsed();
    if !quiet {
        if dry_run {
            println!(
                "🌀 Dry-run successful — simulated {} in {}",
                out_path.display(),
                format_duration(dt).cyan()
            );
        } else {
            let bytes = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
            println!(
                "✨ Generated {} ({} in {}) — with ❤️  from RIM",
                out_path.display().to_string().bold(),
                pretty_bytes(bytes).cyan(),
                format_duration(dt).cyan()
            );
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct DryRunStats {
    logical_bytes: u64,
    allocated_bytes: u64,
    allocated_pages: usize,
}

fn build_config_dry_run(
    layout: &LayoutConfig,
    options: BuildOptions,
    on_event: &mut dyn for<'a> FnMut(BuildEvent<'a>),
) -> anyhow::Result<DryRunStats> {
    let raw_len = raw_image_len(layout, options)?;
    let mut io = SparseRimIO::new(raw_len);

    rimgen::build_config_on_io_with_options_and_events(layout, &mut io, options, on_event)?;

    Ok(DryRunStats {
        logical_bytes: raw_len,
        allocated_bytes: io.allocated_bytes(),
        allocated_pages: io.allocated_pages(),
    })
}

fn build_config_to_file(
    layout: &LayoutConfig,
    output: &PathBuf,
    build_options: BuildOptions,
    on_event: &mut dyn for<'a> FnMut(BuildEvent<'a>),
) -> anyhow::Result<()> {
    let raw_len = raw_image_len(layout, build_options)?;
    let format = image_format_from_path(output)?;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(output)?;

    let mut file_io = FileRimIO::new(file);

    if format == ImageFormat::Raw {
        file_io.set_len(raw_len)?;

        rimgen::build_config_on_io_with_options_and_events(
            layout,
            &mut file_io,
            build_options,
            on_event,
        )?;
    } else {
        let options = rimimg::ImageOptions::default();
        let mut image = rimimg::create_image_io(&mut file_io, raw_len, format, options)?;

        rimgen::build_config_on_io_with_options_and_events(
            layout,
            &mut image,
            build_options,
            on_event,
        )?;

        image.finish()?;
    }

    Ok(())
}

fn raw_image_len(layout: &LayoutConfig, options: BuildOptions) -> anyhow::Result<u64> {
    let sectors = rimgen::calculate_total_disk_sectors_from_config_with_options(layout, options)?;
    sectors
        .checked_mul(rimgen::layout::constants::DEFAULT_SECTOR_SIZE)
        .ok_or_else(|| anyhow!("disk image size overflow"))
}

fn image_format_from_path(path: &Path) -> anyhow::Result<ImageFormat> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    Ok(ImageFormat::from_extension(ext)?)
}
