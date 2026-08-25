// SPDX-License-Identifier: MIT

mod cli;
mod commands;
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
            verbose,
            quiet,
        } => {
            commands::generate::run(layout, output, truncate, dry_run, host, verbose, quiet)?;
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
    }

    Ok(())
}
