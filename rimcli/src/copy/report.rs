// SPDX-License-Identifier: MIT

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyWarningKind {
    UnsupportedSymlink {
        path: String,
        target: String,
        reason: String,
    },
    CaseCollision {
        directory: String,
        entry: String,
        existing: String,
    },
    SkippedFile {
        path: String,
        reason: String,
    },
    MetadataDropped {
        path: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyWarning {
    pub kind: CopyWarningKind,
    pub message: String,
}

impl CopyWarning {
    pub fn new(kind: CopyWarningKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// Execution report of a logical copy operation.
#[derive(Debug, Default, Clone)]
pub struct CopyReport {
    pub directories_created: u64,
    pub files_copied: u64,
    pub symlinks_created: u64,
    pub bytes_transferred: u64,
    pub warnings: Vec<CopyWarning>,
    pub duration: Duration,
}

impl CopyReport {
    pub fn is_success(&self) -> bool {
        true
    }

    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}
