// rimgen/src/main.rs

mod output;

use clap::{Parser, Subcommand};
use rimgen_output::*;
use rimgen_layout::*;
use std::path::PathBuf;

use crate::output::Output;

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
        } => {
            let layout_path = layout;
            let layout = Layout::from_file(&layout_path)?;
            layout.validate()?;
            layout.print_summary();
            if dry_run {
                println!("[rimgen] Dry run mode: no data will be written.");
            } else {
                println!("[rimgen] Writing disk image to: {}", output.display());
            }
            match Output::from_path(&output)? {
                Output::Img => img::create(&layout, &output, &truncate)?,
                Output::Vhd => vhd::create(&layout, &output, &truncate)?,
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
