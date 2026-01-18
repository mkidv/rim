// SPDX-License-Identifier: MIT

mod layout;
mod out;
#[macro_use]
mod utils;

#[cfg(feature = "host-scripts")]
mod host;

use crate::{
    layout::Layout,
    out::{target::DryRunMode, *},
};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, time::Instant};

use crate::utils::log::LogLevel;
use colored::Colorize;

#[derive(Parser)]
#[command(
    name = "rimgen",
    version,
    about = "Rust Image Maker Generator",
    long_about = "rimgen is a declarative disk image generator.\n\nIt automates the creation of partitioned and formatted disk images with file injection, suitable for OS testing, embedded systems flashing, and bootable media creation."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a disk image from a declarative layout file.
    ///
    /// Supported input: TOML layout files.
    /// Supported output formats: Raw (.img), VHD (.vhd), VMDK (.vmdk), QCOW2 (.qcow2), VDI (.vdi).
    Build {
        /// Layout configuration file path (TOML)
        #[arg(short, long, default_value = "layout/layout.toml")]
        layout: PathBuf,

        /// Output image structure. Extension determines format: .img, .vhd, .vmdk, .qcow2, .vdi
        #[arg(short, long, default_value = "output.img")]
        output: PathBuf,

        /// Simulate the build process without writing any data to disk
        #[arg(long)]
        dry_run: bool,

        /// Overwrite existing output file if it exists
        #[arg(long)]
        truncate: bool,

        /// Increase logging verbosity (-v, -vv)
        #[arg(long, short, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Suppress all output except errors
        #[arg(long, short)]
        quiet: bool,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build {
            layout,
            output,
            dry_run,
            truncate,
            verbose,
            quiet,
        } => {
            if quiet && !dry_run {
                crate::utils::set_log_level(LogLevel::Quiet);
            } else if verbose > 0 || dry_run {
                crate::utils::set_log_level(LogLevel::Verbose);
            }
            let t0 = Instant::now();
            crate::log_info!(
                "{}",
                format!("🚀 Rust Image Maker — v{}", env!("CARGO_PKG_VERSION")).bold()
            );

            if dry_run {
                crate::log_normal!("🌀 Dry run mode: no data will be written.");
            } else {
                crate::log_info!("Writing disk image to {}", output.display());
            }

            let layout_path = layout;
            let layout = Layout::from_file(&layout_path)?;
            layout.validate()?;
            crate::log_verbose!("Parsed layout {layout}");

            let out_kind = Output::from_path(&output)?;

            let res = match out_kind {
                Output::Img => img::create(
                    &layout,
                    &output,
                    &truncate,
                    if dry_run {
                        DryRunMode::Tempfile
                    } else {
                        DryRunMode::Off
                    },
                ),
                Output::Qcow2 => qcow2::create(
                    &layout,
                    &output,
                    &truncate,
                    if dry_run {
                        DryRunMode::Tempfile
                    } else {
                        DryRunMode::Off
                    },
                ),
                Output::Vdi => vdi::create(
                    &layout,
                    &output,
                    &truncate,
                    if dry_run {
                        DryRunMode::Tempfile
                    } else {
                        DryRunMode::Off
                    },
                ),
                Output::Vhd => vhd::create(
                    &layout,
                    &output,
                    &truncate,
                    if dry_run {
                        DryRunMode::Tempfile
                    } else {
                        DryRunMode::Off
                    },
                ),
                Output::Vmdk => vmdk::create(
                    &layout,
                    &output,
                    &truncate,
                    if dry_run {
                        DryRunMode::Tempfile
                    } else {
                        DryRunMode::Off
                    },
                ),
            };

            let dt = t0.elapsed().as_secs_f32();
            if let Err(e) = res {
                let _ = std::fs::remove_file(&output);

                crate::log_normal!(
                    "❌ Failed to write {} in {:.2}s\n  ↳ {}",
                    &output.display(),
                    dt,
                    e
                );
                std::process::exit(1);
            } else if dry_run {
                crate::log_normal!(
                    "🌀 Dry-run successful — simulated image {} in {:.2}s (no bytes written)",
                    output.display(),
                    dt
                );
            } else {
                let bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
                crate::log_normal!(
                    "✨ Wrote {} ({} in {}s) — with ❤️  from RIM",
                    output.display().to_string().bold(),
                    utils::pretty_bytes(bytes).cyan(),
                    format!("{dt:.2}").yellow()
                );
            }
        }
    }

    Ok(())
}
