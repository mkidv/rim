// SPDX-License-Identifier: MIT

use serde::{Deserialize, Deserializer};
use std::{fs, path::Path};

#[derive(Debug, PartialEq, Clone)]
pub enum Size {
    Auto,
    Fixed(u64),
}
impl<'de> Deserialize<'de> for Size {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SizeVisitor;

        impl<'de> serde::de::Visitor<'de> for SizeVisitor {
            type Value = Size;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a size string like '512M', '1G', '128K' or 'auto'")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                parse_size_mb(value).map(Size::Fixed).or_else(|_| {
                    if value.trim().eq_ignore_ascii_case("auto") {
                        Ok(Size::Auto)
                    } else {
                        Err(E::custom(format!(
                            "Invalid size format '{value}'. Use K, M or G suffix."
                        )))
                    }
                })
            }
        }

        deserializer.deserialize_str(SizeVisitor)
    }
}

impl std::fmt::Display for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Size::Auto => write!(f, "auto"),
            Size::Fixed(mb) => write!(f, "{mb} MB"),
        }
    }
}

fn parse_size_mb(size: &str) -> anyhow::Result<u64> {
    let lower = size.trim().to_lowercase();

    if let Some(num) = lower.strip_suffix("k") {
        let kb = num.trim().parse::<u64>()?;
        Ok(((kb as f64) / 1024.0).ceil() as u64)
    } else if let Some(num) = lower.strip_suffix("m") {
        Ok(num.trim().parse::<u64>()?)
    } else if let Some(num) = lower.strip_suffix("g") {
        Ok(num.trim().parse::<u64>()? * 1024)
    } else {
        anyhow::bail!("Unknown size format '{}'", size);
    }
}

pub fn calculate_needed_bytes<P: AsRef<Path>>(dir: P) -> anyhow::Result<u64> {
    // Heuristic constants for auto-sizing
    // improved to avoid "No space left on device" errors.
    const BLOCK_SIZE: u64 = 4096;
    const OVERHEAD_FACTOR: f64 = 1.10; // 10% overhead for metadata, tables, journals
    const FIXED_SLACK: u64 = 16 * 1024 * 1024; // 16MB fixed slack for robustness

    fn accumulate(path: &Path) -> anyhow::Result<u64> {
        if path.is_file() {
            let len = fs::metadata(path)?.len();
            // Round up to block size to account for slack space
            let blocks = len.div_ceil(BLOCK_SIZE);
            Ok(blocks * BLOCK_SIZE)
        } else if path.is_dir() {
            let mut total = BLOCK_SIZE; // Assumes a directory takes at least one block
            for entry in fs::read_dir(path)? {
                let entry = match entry {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                total += accumulate(&entry.path()).unwrap_or(0);
            }
            Ok(total)
        } else {
            Ok(0)
        }
    }

    let raw_needed = accumulate(dir.as_ref())?;
    // Apply safety factors
    let with_overhead = (raw_needed as f64 * OVERHEAD_FACTOR) as u64;
    let total = with_overhead + FIXED_SLACK;

    Ok(total)
}
