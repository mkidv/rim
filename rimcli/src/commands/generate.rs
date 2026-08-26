// SPDX-License-Identifier: MIT

use crate::ui::badge::fs_badge;
use crate::ui::format::{format_duration, pretty_bytes, sep_u64};
use crate::ui::progress::create_spinner;
use crate::ui::table::print_layout_table;
use colored::Colorize;
use rimgen::{BuildEvent, DiskLayout, DryRunMode, ImageBuilder};
use std::path::PathBuf;
use std::time::Instant;

pub fn run(
    layout_path: PathBuf,
    output: Option<PathBuf>,
    truncate: bool,
    dry_run: bool,
    host: bool,
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

    let layout = DiskLayout::from_file(&layout_path)?;
    layout.validate()?;

    let out_path = output.unwrap_or_else(|| layout_path.with_extension("img"));

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
    }

    let dry_mode = if dry_run {
        DryRunMode::Tempfile
    } else {
        DryRunMode::Off
    };

    if host {
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

        let mut builder = ImageBuilder::new(layout)
            .truncate(truncate)
            .dry_mode(dry_mode)
            .on_event(move |event| {
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
            });

        builder.build_to_file(&out_path)?;

        if let Some(sp) = spinner {
            sp.finish_and_clear();
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
