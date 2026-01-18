use std::path::{Path, PathBuf};

use rimio::prelude::*;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum DryRunMode {
    /// Nothing is written — only the plan is logged.
    Plan,
    /// Writes to a temporary sparse file, deleted at the end of the run.
    Tempfile,
    /// No dry-run.
    Off,
}

pub struct TargetImage {
    /// Keep ownership for the duration of the run.
    file: Option<std::fs::File>,
    /// If tempfile, keep it to prevent it from being unlinked before the end.
    _tmp: Option<tempfile::NamedTempFile>,
    /// Real path if "Off", otherwise tempfile path (useful for reopening).
    pub path: std::path::PathBuf,

    pub mode: DryRunMode,
}

impl TargetImage {
    pub fn open(output: &Path, total_bytes: u64, mode: DryRunMode) -> anyhow::Result<Self> {
        match mode {
            DryRunMode::Plan => {
                // No file: return a "virtual" handle
                Ok(Self {
                    file: None,
                    _tmp: None,
                    path: PathBuf::new(),
                    mode,
                })
            }
            DryRunMode::Tempfile => {
                let tmp = tempfile::NamedTempFile::new()?;
                let f = tmp.reopen()?; // independent of the "tmp" handle for set_len
                f.set_len(total_bytes)?;
                // Hint: on most modern FS, set_len produces a sparse file
                let path = tmp.path().to_path_buf();
                Ok(Self {
                    file: Some(f),
                    _tmp: Some(tmp),
                    path,
                    mode,
                })
            }
            DryRunMode::Off => {
                let f = std::fs::File::options()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(output)?;
                f.set_len(total_bytes)?;
                Ok(Self {
                    file: Some(f),
                    _tmp: None,
                    path: output.to_path_buf(),
                    mode,
                })
            }
        }
    }

    pub fn as_io<'a>(&'a mut self) -> anyhow::Result<StdRimIO<'a, std::fs::File>> {
        let file = self.file.as_mut().ok_or_else(|| {
            anyhow::anyhow!("No file backing in this mode (Plan). Use Tempfile or Off.")
        })?;
        Ok(StdRimIO::new(file))
    }
}
