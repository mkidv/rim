// SPDX-License-Identifier: MIT

use rimfs_core::errors::{FsInjectorError, FsResolverError};
use std::fmt;

#[derive(Debug)]
pub enum CopyError {
    Resolver {
        path: String,
        source: FsResolverError,
    },
    Injector {
        path: String,
        source: FsInjectorError,
    },
    CaseCollision {
        directory: String,
        entry: String,
        existing: String,
    },
    DestinationExists {
        path: String,
    },
    UnsupportedFeature {
        path: String,
        details: String,
    },
    IO(std::io::Error),
    Other(String),
}

impl fmt::Display for CopyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolver { path, source } => {
                write!(f, "Resolver error at '{path}': {source}")
            }
            Self::Injector { path, source } => {
                write!(f, "Injector error at '{path}': {source}")
            }
            Self::CaseCollision {
                directory,
                entry,
                existing,
            } => {
                write!(
                    f,
                    "Case collision in directory '{directory}': entry '{entry}' conflicts with '{existing}'"
                )
            }
            Self::DestinationExists { path } => {
                write!(f, "Destination already exists: '{path}'")
            }
            Self::UnsupportedFeature { path, details } => {
                write!(f, "Unsupported feature at '{path}': {details}")
            }
            Self::IO(e) => write!(f, "I/O error: {e}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for CopyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolver { source, .. } => Some(source),
            Self::Injector { source, .. } => Some(source),
            Self::IO(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CopyError {
    fn from(e: std::io::Error) -> Self {
        Self::IO(e)
    }
}

pub type CopyResult<T = ()> = Result<T, CopyError>;
