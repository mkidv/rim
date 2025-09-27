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

#[derive(Parser)]
#[command(name = "rimgen", version, about = "Rust Image Generator", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a disk image from layout.toml
    Build {
        /// Layout path
        #[arg(short, long, default_value = "layout/layout.toml")]
        layout: PathBuf,
        /// Output path
        #[arg(short, long, default_value = "output.img")]
        output: PathBuf,

        /// Only print what would be done, don't write the image
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        truncate: bool,
        #[arg(long, short, action = clap::ArgAction::Count)]
        verbose: u8,

        #[arg(long, short)]
        quiet: bool,
    },
    /// Flash image to a physical device
    Flash {
        /// Layout path
        #[arg(short, long, default_value = "layout/layout.toml")]
        layout: PathBuf,

        /// Target block device (e.g., /dev/sdX, \\.\PhysicalDrive1)
        #[arg(short, long)]
        device: PathBuf,

        /// Only print what would be done, don't write the image
        #[arg(long)]
        dry_run: bool,

        /// Require confirmation to flash
        #[arg(long, default_value_t = true)]
        no_confirm: bool,

        /// Compare written image with device
        #[arg(long)]
        no_verify: bool,
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
            crate::log_info!("🚀 Rust Image Maker — v{}", env!("CARGO_PKG_VERSION"));

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
            } else if (dry_run) {
                crate::log_normal!(
                    "🌀 Dry-run successful — simulated image {} in {:.2}s (no bytes written)",
                    output.display(),
                    dt
                );
            } else {
                let bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);
                crate::log_normal!(
                    "✨ Wrote {} ({} in {:.2}s) — with ❤️  from RIM",
                    output.display(),
                    utils::pretty_bytes(bytes),
                    dt
                );
            }
        }
        Commands::Flash {
            layout,
            device,
            dry_run,
            no_confirm,
            no_verify,
        } => {}
    }

    Ok(())
}
