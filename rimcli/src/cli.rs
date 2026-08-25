// SPDX-License-Identifier: MIT

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rim",
    version,
    about = "Rust Image Maker — Unified disk image engine & CLI",
    long_about = "Declarative disk image generation, format conversion, inspection, and verification tool."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Generate a disk image from a declarative layout (aliases: build, gen)
    #[command(alias = "build", alias = "gen")]
    Generate {
        /// Path to declarative layout TOML file
        layout: PathBuf,

        /// Output file path (format auto-detected by extension: .img, .vhd, .vmdk, .qcow2, .vdi)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Truncate image size to minimal usable GPT blocks
        #[arg(short, long)]
        truncate: bool,

        /// Simulate build without writing bytes (or plan)
        #[arg(short = 'n', long)]
        dry_run: bool,

        /// Use OS-native tools (via rimhost) instead of pure-Rust engine
        #[arg(long)]
        host: bool,

        /// Verbose output
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Quiet output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Convert an existing disk image between container formats (.img, .vhd, .vmdk, .qcow2, .vdi)
    Convert {
        /// Input image file
        input: PathBuf,

        /// Output image file
        output: PathBuf,

        /// Verbose output
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,

        /// Quiet output
        #[arg(short, long)]
        quiet: bool,
    },

    /// Check and verify partition tables and filesystem structures in a disk image
    Check {
        /// Target disk image file
        image: PathBuf,

        /// Verbose output
        #[arg(short, long, action = clap::ArgAction::Count)]
        verbose: u8,
    },

    /// Display partition table (GPT / MBR) of a disk image
    Partition {
        /// Target disk image file
        image: PathBuf,
    },

    /// Inspect a disk image container format, partition scheme, and filesystems
    Inspect {
        /// Target disk image file
        image: PathBuf,
    },
}
