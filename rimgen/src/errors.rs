// SPDX-License-Identifier: MIT

//! Declarative disk image generation errors.

use crate::layout::{Filesystem, PartitionKind};
use alloc::string::String;
use core::fmt;
use rimfs::FsError;
use rimio::RimIOError;
use rimpart::PartError;

pub type GenResult<T = ()> = Result<T, GenError>;
pub type LayoutResult<T = ()> = Result<T, LayoutError>;

/// Unified top-level error type for `rimgen`.
#[derive(Debug)]
pub enum GenError {
    IO(RimIOError),
    Fs(FsError),
    Part(PartError),
    Layout(LayoutError),
    #[cfg(feature = "std")]
    StdIo(std::io::Error),
    #[cfg(feature = "std")]
    Toml(toml::de::Error),
    UnsupportedFs(Filesystem),
    PartitionDoesNotFit {
        name: String,
        end_lba: u64,
        total_sectors: u64,
    },
    PayloadTooLarge {
        path: String,
        part_name: String,
        payload_bytes: u64,
        part_bytes: u64,
    },
    TargetError(String),
    ContainerError(String),
    Other(&'static str),
    Custom(String),
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::IO(e) => write!(f, "I/O error: {}", e.msg()),
            GenError::Fs(e) => write!(f, "Filesystem error: {e}"),
            GenError::Part(e) => write!(f, "Partitioning error: {e}"),
            GenError::Layout(e) => write!(f, "Layout error: {e}"),
            #[cfg(feature = "std")]
            GenError::StdIo(e) => write!(f, "System I/O error: {e}"),
            #[cfg(feature = "std")]
            GenError::Toml(e) => write!(f, "TOML parsing error: {e}"),
            GenError::UnsupportedFs(fs) => write!(
                f,
                "Filesystem '{fs}' is not supported by pure-Rust builder. For OS-native formatting, use rimhost."
            ),
            GenError::PartitionDoesNotFit {
                name,
                end_lba,
                total_sectors,
            } => write!(
                f,
                "Partition '{name}' does not fit on disk (end sector {end_lba} >= total sectors {total_sectors})"
            ),
            GenError::PayloadTooLarge {
                path,
                part_name,
                payload_bytes,
                part_bytes,
            } => write!(
                f,
                "Payload '{path}' ({payload_bytes} bytes) is too large for partition '{part_name}' ({part_bytes} bytes)"
            ),
            GenError::TargetError(msg) => write!(f, "Target disk error: {msg}"),
            GenError::ContainerError(msg) => write!(f, "Container conversion error: {msg}"),
            GenError::Other(msg) => write!(f, "{msg}"),
            GenError::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for GenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GenError::StdIo(e) => Some(e),
            GenError::Toml(e) => Some(e),
            GenError::Layout(e) => Some(e),
            _ => None,
        }
    }
}

/// Errors occurring during layout definition, parsing, or validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    SizeTooLarge {
        fs: Filesystem,
        size_mb: u64,
        limit_mb: u64,
    },
    SizeTooSmall {
        fs: Filesystem,
        size_mb: u64,
        min_mb: u64,
    },
    InvalidSizeFormat(String),
    InvalidConfig(&'static str),
    MissingGuid(String),
    RequiresExplicitKind {
        name: String,
        kind: PartitionKind,
    },
    MountpointOnNonMountable {
        name: String,
        fs: Filesystem,
    },
    InvalidAlignment {
        bytes: u64,
        sector_size: u64,
    },
    InvalidUuid(String),
    Other(&'static str),
    Custom(String),
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutError::SizeTooLarge {
                fs,
                size_mb,
                limit_mb,
            } => write!(
                f,
                "{fs} is not recommended beyond {limit_mb} MiB (got {size_mb} MiB)"
            ),
            LayoutError::SizeTooSmall {
                fs,
                size_mb,
                min_mb,
            } => write!(f, "{fs} needs at least {min_mb} MiB (got {size_mb} MiB)"),
            LayoutError::InvalidSizeFormat(s) => {
                write!(f, "Invalid size format '{s}'. Use K, M, or G suffix.")
            }
            LayoutError::InvalidConfig(msg) => write!(f, "Invalid configuration: {msg}"),
            LayoutError::MissingGuid(name) => write!(
                f,
                "Partition '{name}' is mountable but has no GUID assigned. Call `assign_guids()`."
            ),
            LayoutError::RequiresExplicitKind { name, kind } => write!(
                f,
                "Partition '{name}' requires explicit type '{kind:?}' but none was provided."
            ),
            LayoutError::MountpointOnNonMountable { name, fs } => write!(
                f,
                "Partition '{name}' has a mountpoint but filesystem '{fs}' is non-mountable (Raw/None)."
            ),
            LayoutError::InvalidAlignment { bytes, sector_size } => write!(
                f,
                "Alignment {bytes} bytes is not a multiple of sector size ({sector_size})"
            ),
            LayoutError::InvalidUuid(s) => write!(f, "Invalid UUID / Volume ID: '{s}'"),
            LayoutError::Other(msg) => write!(f, "{msg}"),
            LayoutError::Custom(msg) => write!(f, "{msg}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LayoutError {}

crate::gen_error_wiring! {
    top => GenError {
        RimIOError  : IO,
        FsError     : Fs,
        PartError   : Part,
        LayoutError : Layout,
    },
    str_into => [
        LayoutError,
    ],
    sub => {
        rimfs::core::errors::FsResolverError => [GenError::Fs],
    },
}

#[cfg(feature = "std")]
impl From<std::io::Error> for GenError {
    fn from(e: std::io::Error) -> Self {
        GenError::StdIo(e)
    }
}

#[cfg(feature = "std")]
impl From<toml::de::Error> for GenError {
    fn from(e: toml::de::Error) -> Self {
        GenError::Toml(e)
    }
}

impl From<String> for GenError {
    fn from(s: String) -> Self {
        GenError::Custom(s)
    }
}

impl From<String> for LayoutError {
    fn from(s: String) -> Self {
        LayoutError::Custom(s)
    }
}
