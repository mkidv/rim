// SPDX-License-Identifier: MIT

//! Endpoint parsing for paths targeting host, disk images, or partitions.

use std::path::PathBuf;

/// Represents an endpoint for `rim copy` (source or destination).
///
/// Supported endpoint formats:
/// - Plain host path: `host/path`, `/path/to/dir`, `C:\path\to\dir`
/// - Image with partition and internal path: `disk.img:1:/EFI/BOOT`, `C:\disk.img:2:/etc`
/// - Image with partition only: `disk.img:1`
/// - Unpartitioned image or archive with internal path: `rootfs.ext4:/etc`, `archive.tar:/docs`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyEndpoint {
    /// Filesystem path on the host to the file, directory, or disk image container.
    pub host_path: PathBuf,
    /// 1-based partition selector (1, 2, ...), if specified.
    pub partition: Option<usize>,
    /// Internal subpath within the filesystem/archive/image. Normalized (e.g. "/", "/etc", "/EFI/BOOT").
    pub internal_path: String,
    /// Whether the endpoint explicitly used image/partition delimiter syntax (`:`).
    pub is_explicit_image: bool,
}

impl CopyEndpoint {
    /// Parses an endpoint string into a [CopyEndpoint].
    ///
    /// Correctly identifies Windows drive letters (`C:\`, `C:/`, `\\?\C:\`) to prevent
    /// them from being mistaken for endpoint delimiters.
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            anyhow::bail!("Endpoint path cannot be empty");
        }

        let prefix_len = Self::windows_path_prefix_len(trimmed);
        let search_str = &trimmed[prefix_len..];

        // Find all ':' delimiters after any Windows drive prefix
        let delimiter_indices: Vec<usize> = search_str
            .match_indices(':')
            .map(|(i, _)| prefix_len + i)
            .collect();

        if delimiter_indices.is_empty() {
            // Plain host path (directory or unpartitioned image/archive without colon)
            return Ok(Self {
                host_path: PathBuf::from(trimmed),
                partition: None,
                internal_path: "/".to_string(),
                is_explicit_image: false,
            });
        }

        let first_col = delimiter_indices[0];
        let host_str = &trimmed[..first_col];
        if host_str.is_empty() {
            anyhow::bail!("Invalid endpoint '{trimmed}': image path before ':' cannot be empty");
        }

        let after_first_col = &trimmed[first_col + 1..];

        if delimiter_indices.len() >= 2 {
            let second_col = delimiter_indices[1];
            let middle_part = &trimmed[first_col + 1..second_col];
            let remainder_path = &trimmed[second_col + 1..];

            // If middle_part parses as a positive integer, it's an explicit partition selector
            match middle_part.parse::<usize>() {
                Ok(0) => {
                    anyhow::bail!(
                        "Invalid partition number 0 in endpoint '{trimmed}': partition numbers are 1-based (use 1 for the first partition)"
                    );
                }
                Ok(part_num) => {
                    let norm_path = Self::normalize_internal_path(remainder_path);
                    return Ok(Self {
                        host_path: PathBuf::from(host_str),
                        partition: Some(part_num),
                        internal_path: norm_path,
                        is_explicit_image: true,
                    });
                }
                Err(_) => {
                    // Middle part is not a number. If it started with '/' or '\', then everything after
                    // first_col is internal path (e.g. disk.img:/dir:with:colon)
                    if middle_part.starts_with('/') || middle_part.starts_with('\\') {
                        let norm_path = Self::normalize_internal_path(after_first_col);
                        return Ok(Self {
                            host_path: PathBuf::from(host_str),
                            partition: None,
                            internal_path: norm_path,
                            is_explicit_image: true,
                        });
                    }

                    // Otherwise user entered an invalid partition selector like `disk.img:p1:/etc`
                    anyhow::bail!(
                        "Invalid partition selector '{middle_part}' in endpoint '{trimmed}': partition must be a 1-based integer (e.g. disk.img:1:/path)"
                    );
                }
            }
        }

        // Exactly one colon in `search_str`
        // Could be `<image>:<partition_number>` or `<image>:<internal_path>`
        match after_first_col.parse::<usize>() {
            Ok(0) => {
                anyhow::bail!(
                    "Invalid partition number 0 in endpoint '{trimmed}': partition numbers are 1-based (use 1 for the first partition)"
                );
            }
            Ok(part_num) => Ok(Self {
                host_path: PathBuf::from(host_str),
                partition: Some(part_num),
                internal_path: "/".to_string(),
                is_explicit_image: true,
            }),
            Err(_) => {
                let norm_path = Self::normalize_internal_path(after_first_col);
                Ok(Self {
                    host_path: PathBuf::from(host_str),
                    partition: None,
                    internal_path: norm_path,
                    is_explicit_image: true,
                })
            }
        }
    }

    /// Normalizes internal virtual filesystem paths to leading `/` format.
    pub fn normalize_internal_path(p: &str) -> String {
        let p = p.trim();
        if p.is_empty() || p == "/" || p == "\\" {
            return "/".to_string();
        }
        let converted = p.replace('\\', "/");
        if converted.starts_with('/') {
            converted
        } else {
            format!("/{converted}")
        }
    }

    /// Detects Windows drive prefixes (e.g. `C:\`, `c:/`, `\\?\C:\`) and returns the byte offset
    /// after the drive colon to prevent it from being parsed as an endpoint delimiter.
    pub fn windows_path_prefix_len(s: &str) -> usize {
        let bytes = s.as_bytes();
        // Standard DOS drive prefix: `C:\` or `C:/`
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && (bytes[2] == b'/' || bytes[2] == b'\\')
        {
            return 2;
        }

        // Verbatim UNC drive prefix: `\\?\C:\` or `\\.\C:\`
        if (s.starts_with(r"\\?\") || s.starts_with(r"\\.\"))
            && bytes.len() >= 7
            && bytes[4].is_ascii_alphabetic()
            && bytes[5] == b':'
            && (bytes[6] == b'/' || bytes[6] == b'\\')
        {
            return 6;
        }

        0
    }
}

#[cfg(test)]
#[path = "endpoint/tests.rs"]
mod tests;
