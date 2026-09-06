// SPDX-License-Identifier: MIT

use std::str::FromStr;

/// Policy for handling entry metadata (timestamps, unix permissions/modes, ownership).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataPolicy {
    /// Preserve all representable metadata (timestamps, unix mode, read-only).
    #[default]
    PreserveAll,
    /// Preserve basic metadata (timestamps), strip unix ownership and mode bits.
    PreserveBasic,
    /// Strip all metadata, let the destination filesystem assign defaults.
    Strip,
}

impl FromStr for MetadataPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "preserve-all" | "all" | "preserve" => Ok(Self::PreserveAll),
            "preserve-basic" | "basic" | "best-effort" => Ok(Self::PreserveBasic),
            "strip" | "none" | "ignore" => Ok(Self::Strip),
            other => Err(format!(
                "Unknown metadata policy '{other}'. Supported: preserve-all, preserve-basic, strip"
            )),
        }
    }
}

/// Policy for handling unsupported destination features (such as symlinks on FAT/exFAT).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnsupportedMetadataPolicy {
    /// Abort the copy operation with an error when an unsupported feature is encountered.
    Error,
    /// Log a warning and skip the unsupported entry, continuing the copy operation.
    #[default]
    Warn,
    /// Silently skip the unsupported entry without logging a warning.
    Ignore,
}

impl FromStr for UnsupportedMetadataPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "warn" | "warning" => Ok(Self::Warn),
            "error" | "fail" => Ok(Self::Error),
            "ignore" | "skip" => Ok(Self::Ignore),
            other => Err(format!(
                "Unknown unsupported feature policy '{other}'. Supported: warn, error, ignore"
            )),
        }
    }
}

/// Policy for handling existing destination files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverwritePolicy {
    /// Overwrite existing destination files.
    Replace,
    /// Abort if destination file already exists.
    #[default]
    Error,
    /// Skip copying if destination file already exists.
    Skip,
}

impl FromStr for OverwritePolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "replace" | "overwrite" => Ok(Self::Replace),
            "error" | "fail" => Ok(Self::Error),
            "skip" => Ok(Self::Skip),
            other => Err(format!(
                "Unknown overwrite policy '{other}'. Supported: replace, error, skip"
            )),
        }
    }
}

/// Configuration options for the logical copy engine.
#[derive(Debug, Clone)]
pub struct CopyOptions {
    /// Metadata preservation strategy.
    pub metadata_policy: MetadataPolicy,
    /// Handling of unsupported destination capabilities.
    pub unsupported_policy: UnsupportedMetadataPolicy,
    /// Overwrite behavior for destination conflicts.
    pub overwrite_policy: OverwritePolicy,
    /// Whether to check sibling entries for case-insensitive collisions.
    pub detect_case_collisions: bool,
    /// Whether the destination filesystem is case-sensitive (e.g. EXT4, Tar, Zip vs FAT, exFAT, NTFS).
    pub destination_case_sensitive: bool,
    /// Whether the destination injector supports in-place replacement (true only for Host / StdInjector).
    pub destination_supports_replace: bool,
}

impl Default for CopyOptions {
    fn default() -> Self {
        Self {
            metadata_policy: MetadataPolicy::default(),
            unsupported_policy: UnsupportedMetadataPolicy::default(),
            overwrite_policy: OverwritePolicy::default(),
            detect_case_collisions: true,
            destination_case_sensitive: true,
            destination_supports_replace: false,
        }
    }
}
