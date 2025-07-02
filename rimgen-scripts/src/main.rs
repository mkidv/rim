#[macro_use]
mod args;
mod cmd_builder;
mod platform;

use anyhow::Result;
use clap::Parser;
use rimgen_layout::Layout;
use std::path::PathBuf;

/// Rimgen-Scripts CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to layout TOML
    #[arg(short, long, default_value = "layout/layout.toml")]
    layout: PathBuf,

    /// Path to image (IMG or VHD)
    #[arg(short, long, default_value = "output.img")]
    output: PathBuf,

    /// Enable debug print of script
    #[arg(long, default_value_t = false)]
    debug: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    println!("[rimgen-scripts] Loading layout from {:?}", cli.layout);
    let layout_path = cli.layout;
    let layout = Layout::from_file(&layout_path)?;
    layout.validate()?;
    layout.print_summary();

    println!("[rimgen-scripts] Injecting to {:?}", cli.output);

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let is_vhd = cli
            .output
            .extension()
            .map(|e| e.eq_ignore_ascii_case("vhd"))
            .unwrap_or(false);
        if is_vhd {
            anyhow::bail!("VHD injection is only supported on Windows.");
        }
    }

    // Run inject (calls appropriate platform::*::inject)
    platform::inject(&layout, &cli.output, cli.debug)?;

    println!("[rimgen-scripts] Done.");

    Ok(())
}
