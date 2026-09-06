// SPDX-License-Identifier: MIT

pub mod dry_run;
pub mod engine;
pub mod error;
pub mod options;
pub mod progress;
pub mod report;

pub use dry_run::DryRunStdInjector;
pub use engine::copy_tree;
pub use error::{CopyError, CopyResult};
pub use options::{CopyOptions, MetadataPolicy, OverwritePolicy, UnsupportedMetadataPolicy};
pub use progress::{CopyEvent, ProgressCallback};
pub use report::{CopyReport, CopyWarning, CopyWarningKind};
