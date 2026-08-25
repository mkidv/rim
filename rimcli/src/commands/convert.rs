// SPDX-License-Identifier: MIT

use crate::ui::{format_duration, pretty_bytes};
use colored::Colorize;
use std::path::PathBuf;
use std::time::Instant;

pub fn run(input: PathBuf, output: PathBuf, _verbose: u8, quiet: bool) -> anyhow::Result<()> {
    let t0 = Instant::now();

    if !quiet {
        println!(
            "{}",
            format!("🚀 Rust Image Maker — v{}", env!("CARGO_PKG_VERSION")).bold()
        );
        println!(
            "Converting {} → {}",
            input.display().to_string().cyan(),
            output.display().to_string().cyan()
        );
    }

    let input_len = std::fs::metadata(&input).map(|m| m.len()).unwrap_or(0);

    if !quiet && input_len > 0 {
        let pb = crate::ui::create_byte_progress(input_len, "Streaming");
        rimimg::convert_with_progress(&input, &output, |curr, _total| {
            pb.set_position(curr);
        })?;
        pb.finish_and_clear();
    } else {
        rimimg::convert(&input, &output)?;
    }

    let duration = t0.elapsed();
    let out_bytes = std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0);

    if !quiet {
        println!(
            "✨ Converted to {} ({} in {}) — with ❤️  from RIM",
            output.display().to_string().bold(),
            pretty_bytes(out_bytes).cyan(),
            format_duration(duration)
        );
    }

    Ok(())
}
