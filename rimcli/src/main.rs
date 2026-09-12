// SPDX-License-Identifier: MIT

//! Multi-format disk image and filesystem manipulation CLI.

mod cli;
mod commands;
pub mod copy;
pub mod ui;

use clap::Parser;
use cli::{Cli, Commands};

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Generate {
            layout,
            output,
            truncate,
            dry_run,
            host,
            no_gpt,
            verbose,
            quiet,
        } => {
            commands::generate::run(
                layout, output, truncate, dry_run, host, no_gpt, verbose, quiet,
            )?;
        }
        Commands::Convert {
            input,
            output,
            verbose,
            quiet,
        } => {
            commands::convert::run(input, output, verbose, quiet)?;
        }
        Commands::Check { image, verbose } => {
            commands::check::run(image, verbose)?;
        }
        Commands::Partition { image } => {
            commands::partition::run(&image)?;
        }
        Commands::Inspect { image } => {
            commands::inspect::run(&image)?;
        }
        Commands::Copy {
            source,
            destination,
            src_path,
            metadata,
            unsupported,
            overwrite,
            dry_run,
            no_detect_case_collisions,
            verbose,
            quiet,
        } => {
            commands::copy::run(
                source,
                destination,
                src_path,
                metadata,
                unsupported,
                overwrite,
                dry_run,
                no_detect_case_collisions,
                verbose,
                quiet,
            )?;
        }
    }

    Ok(())
}
