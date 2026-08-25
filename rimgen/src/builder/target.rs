// SPDX-License-Identifier: MIT

use crate::errors::{GenError, GenResult};
use rimio::prelude::*;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DryRunMode {
    Off,
    Plan,
    Tempfile,
}

pub(crate) struct TargetImage {
    /// Keep ownership for the duration of the run.
    file: Option<std::fs::File>,
    /// If tempfile, keep it to prevent it from being unlinked before the end.
    _tmp: Option<tempfile::NamedTempFile>,
    /// Real path if "Off", otherwise tempfile path (useful for reopening).
    #[allow(dead_code)]
    pub path: PathBuf,
    #[allow(dead_code)]
    pub mode: DryRunMode,
}

impl TargetImage {
    pub fn open(output: &Path, total_bytes: u64, mode: DryRunMode) -> GenResult<Self> {
        match mode {
            DryRunMode::Plan => Err(GenError::TargetError(
                "Plan mode does not open a target image".to_string(),
            )),
            DryRunMode::Tempfile => {
                let tmp = tempfile::NamedTempFile::new().map_err(|e| {
                    GenError::TargetError(format!("Failed to create tempfile for dry-run: {e}"))
                })?;
                let path = tmp.path().to_path_buf();
                let file = tmp.as_file().try_clone()?;

                let mut target = Self {
                    file: Some(file),
                    _tmp: Some(tmp),
                    path,
                    mode,
                };
                target.set_len(total_bytes)?;
                Ok(target)
            }
            DryRunMode::Off => {
                let file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(output)
                    .map_err(|e| {
                        GenError::TargetError(format!(
                            "Failed to create output disk image '{}': {e}",
                            output.display()
                        ))
                    })?;

                let mut target = Self {
                    file: Some(file),
                    _tmp: None,
                    path: output.to_path_buf(),
                    mode,
                };
                target.set_len(total_bytes)?;
                Ok(target)
            }
        }
    }

    fn set_len(&mut self, total_bytes: u64) -> GenResult<()> {
        if let Some(file) = &mut self.file {
            file.set_len(total_bytes)?;
        }
        Ok(())
    }

    pub fn as_io(&mut self) -> GenResult<StdRimIO<'_, std::fs::File>> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| GenError::TargetError("No active file in TargetImage".to_string()))?;
        Ok(StdRimIO::new(file))
    }
}
