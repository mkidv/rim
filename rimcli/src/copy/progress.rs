// SPDX-License-Identifier: MIT

//! Copy progress tracking events and lifecycle callbacks.

use super::report::CopyWarning;

/// Lifecycle events emitted during a copy operation.
#[derive(Debug, Clone)]
pub enum CopyEvent<'a> {
    StartingDirectory {
        path: &'a str,
    },
    StartingFile {
        path: &'a str,
        size: u64,
    },
    FileProgress {
        path: &'a str,
        bytes_written: u64,
        total_bytes: u64,
    },
    FinishedFile {
        path: &'a str,
        size: u64,
    },
    CreatedSymlink {
        path: &'a str,
        target: &'a str,
    },
    Warning {
        warning: &'a CopyWarning,
    },
}

pub type ProgressCallback<'a> = &'a mut dyn FnMut(CopyEvent<'_>);
