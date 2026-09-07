// SPDX-License-Identifier: MIT

#[cfg(feature = "std")]
use crate::errors::GenResult;
use crate::errors::{LayoutError, LayoutResult};
#[cfg(not(feature = "std"))]
use alloc::string::ToString;
use serde::{Deserialize, Deserializer};
#[cfg(feature = "std")]
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

            fn expecting(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
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
                        Err(E::custom(alloc::format!(
                            "Invalid size format '{value}'. Use K, M or G suffix."
                        )))
                    }
                })
            }
        }

        deserializer.deserialize_str(SizeVisitor)
    }
}

impl core::fmt::Display for Size {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Size::Auto => write!(f, "auto"),
            Size::Fixed(mb) => write!(f, "{mb} MB"),
        }
    }
}

pub fn parse_size_mb(size: &str) -> LayoutResult<u64> {
    let lower = size.trim().to_lowercase();
    let lower = lower.trim_end_matches('b');
    let lower = lower.trim_end_matches('i');

    if let Some(num) = lower.strip_suffix('k') {
        let kb = num
            .trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidSizeFormat(size.to_string()))?;
        Ok(kb.div_ceil(1024))
    } else if let Some(num) = lower.strip_suffix('m') {
        num.trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidSizeFormat(size.to_string()))
    } else if let Some(num) = lower.strip_suffix('g') {
        let gb = num
            .trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidSizeFormat(size.to_string()))?;
        Ok(gb * 1024)
    } else if let Some(num) = lower.strip_suffix('t') {
        let tb = num
            .trim()
            .parse::<u64>()
            .map_err(|_| LayoutError::InvalidSizeFormat(size.to_string()))?;
        Ok(tb * 1024 * 1024)
    } else {
        crate::bail!(LayoutError::InvalidSizeFormat(size.to_string()));
    }
}

#[cfg(feature = "std")]
pub fn calculate_needed_bytes<P: AsRef<Path>>(dir: P) -> GenResult<u64> {
    const BLOCK_SIZE: u64 = 4096;
    const OVERHEAD_FACTOR: f64 = 1.10;
    const FIXED_SLACK: u64 = 16 * 1024 * 1024;

    fn accumulate(path: &Path) -> std::io::Result<u64> {
        if path.is_file() {
            let len = fs::metadata(path)?.len();
            let blocks = len.div_ceil(BLOCK_SIZE);
            Ok(blocks * BLOCK_SIZE)
        } else if path.is_dir() {
            let mut total = BLOCK_SIZE;
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
    let with_overhead = (raw_needed as f64 * OVERHEAD_FACTOR) as u64;
    let total = with_overhead + FIXED_SLACK;

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_mb_suffixes() {
        assert_eq!(parse_size_mb("64M").unwrap(), 64);
        assert_eq!(parse_size_mb("64MB").unwrap(), 64);
        assert_eq!(parse_size_mb("64MiB").unwrap(), 64);
        assert_eq!(parse_size_mb("64mib").unwrap(), 64);
        assert_eq!(parse_size_mb("1G").unwrap(), 1024);
        assert_eq!(parse_size_mb("1GB").unwrap(), 1024);
        assert_eq!(parse_size_mb("1GiB").unwrap(), 1024);
        assert_eq!(parse_size_mb("2T").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size_mb("2TB").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size_mb("2TiB").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_size_mb("512K").unwrap(), 1);
        assert_eq!(parse_size_mb("2048K").unwrap(), 2);
        assert_eq!(parse_size_mb("2048KB").unwrap(), 2);
        assert_eq!(parse_size_mb("2048KiB").unwrap(), 2);
    }
}
