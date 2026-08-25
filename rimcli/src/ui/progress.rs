// SPDX-License-Identifier: MIT

use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// Create a stylish progress bar for streaming conversions and payload copies.
pub fn create_byte_progress(total_bytes: u64, message: &str) -> ProgressBar {
    let pb = ProgressBar::new(total_bytes);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{prefix:.bold} [{elapsed_precise}] [{bar:36.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("━╾─"),
    );
    pb.set_prefix(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));
    pb
}

/// Create a spinner for interactive status updates.
pub fn create_spinner(message: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.green.bold} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.set_message(message.to_string());
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}
