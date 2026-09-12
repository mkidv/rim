// SPDX-License-Identifier: MIT

//! Simulated in-memory copy injector for dry-run validation.

use std::path::{Component, Path, PathBuf};

use rimfs_core::StdOverwritePolicy;
use rimfs_core::errors::{FsInjectorError, FsInjectorResult};
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::resolver::attr::FileAttributes;
use rimio::RimRead;

/// In-memory simulation injector for Host destinations in dry-run mode.
///
/// Implements [FsTreeInjector] without creating or modifying any files or directories on disk.
/// Validates path confinement, entry names, source streaming integrity, and host overwrite policies.
#[derive(Debug, Clone)]
pub struct DryRunStdInjector {
    root_path: PathBuf,
    dir_stack: Vec<PathBuf>,
    overwrite_policy: StdOverwritePolicy,
    skipped_count: u64,
}

impl DryRunStdInjector {
    pub fn new(root_path: impl AsRef<Path>) -> Self {
        let root = root_path.as_ref();
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        Self {
            root_path: canonical_root.clone(),
            dir_stack: vec![canonical_root],
            overwrite_policy: StdOverwritePolicy::default(),
            skipped_count: 0,
        }
    }

    /// Configures the overwrite policy.
    pub fn with_overwrite_policy(mut self, policy: StdOverwritePolicy) -> Self {
        self.overwrite_policy = policy;
        self
    }

    pub fn set_overwrite_policy(&mut self, policy: StdOverwritePolicy) {
        self.overwrite_policy = policy;
    }

    /// Returns the number of files and symlinks skipped due to overwrite policy.
    pub fn skipped_count(&self) -> u64 {
        self.skipped_count
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    fn resolve_child_path(&self, name: &str) -> FsInjectorResult<PathBuf> {
        if name.is_empty() {
            return Err(FsInjectorError::Invalid("Entry name cannot be empty"));
        }

        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return Err(FsInjectorError::Invalid(
                "Entry name cannot contain path separators or NUL",
            ));
        }

        if name.contains(':') {
            return Err(FsInjectorError::Invalid(
                "Entry name cannot contain colon or stream prefixes",
            ));
        }

        let current_dir = self
            .dir_stack
            .last()
            .ok_or(FsInjectorError::Invalid("Directory stack is empty"))?;

        let p = Path::new(name);
        for comp in p.components() {
            match comp {
                Component::Normal(_) => {}
                _ => {
                    return Err(FsInjectorError::Invalid(
                        "Path traversal or non-normal component rejected",
                    ));
                }
            }
        }

        if let Ok(canon_parent) = current_dir.canonicalize() {
            if !canon_parent.starts_with(&self.root_path) {
                return Err(FsInjectorError::Invalid(
                    "Root escape detected: parent directory is outside root",
                ));
            }
        } else if !current_dir.starts_with(&self.root_path) {
            return Err(FsInjectorError::Invalid(
                "Root escape detected: parent path is outside root",
            ));
        }

        let target = current_dir.join(name);

        if let Ok(canon_target) = target.canonicalize()
            && !canon_target.starts_with(&self.root_path)
        {
            return Err(FsInjectorError::Invalid(
                "Root escape detected: existing target resolves outside root",
            ));
        }

        Ok(target)
    }
}

impl FsTreeInjector<()> for DryRunStdInjector {
    fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
        self.dir_stack.clear();
        self.dir_stack.push(self.root_path.clone());
        Ok(())
    }

    fn write_dir(&mut self, name: &str, _attr: &FileAttributes) -> FsInjectorResult {
        let target = self.resolve_child_path(name)?;

        if let Ok(meta) = std::fs::symlink_metadata(&target) {
            if meta.is_symlink() {
                return Err(FsInjectorError::Invalid(
                    "Cannot traverse through existing symlink",
                ));
            }
            if meta.is_file() {
                return Err(FsInjectorError::Invalid(
                    "Cannot replace regular file with directory",
                ));
            }
            if let Ok(canon) = target.canonicalize()
                && !canon.starts_with(&self.root_path)
            {
                return Err(FsInjectorError::Invalid(
                    "Root escape detected: directory resolves outside root",
                ));
            }
        }

        self.dir_stack.push(target);
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        _attr: &FileAttributes,
    ) -> FsInjectorResult {
        let target = self.resolve_child_path(name)?;

        if let Ok(meta) = std::fs::symlink_metadata(&target) {
            if meta.is_symlink() {
                return Err(FsInjectorError::Invalid(
                    "Cannot replace through existing symlink",
                ));
            }
            if meta.is_dir() {
                return Err(FsInjectorError::Invalid(
                    "Cannot replace directory with regular file",
                ));
            }
            match self.overwrite_policy {
                StdOverwritePolicy::Error => {
                    return Err(FsInjectorError::Invalid("Destination file already exists"));
                }
                StdOverwritePolicy::Skip => {
                    self.skipped_count += 1;
                    return Ok(());
                }
                StdOverwritePolicy::Replace => {}
            }
        }

        // Stream source payload to verify data reads cleanly without errors
        let mut buf = [0u8; 64 * 1024];
        let mut offset = 0u64;
        let mut remaining = size;
        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            source.read_at(offset, &mut buf[..to_read])?;
            offset += to_read as u64;
            remaining -= to_read as u64;
        }

        Ok(())
    }

    fn write_symlink(
        &mut self,
        name: &str,
        _target: &str,
        _attr: &FileAttributes,
    ) -> FsInjectorResult {
        let link_path = self.resolve_child_path(name)?;

        if let Ok(meta) = std::fs::symlink_metadata(&link_path) {
            if meta.is_dir() {
                return Err(FsInjectorError::Invalid(
                    "Cannot replace directory with symlink",
                ));
            }
            match self.overwrite_policy {
                StdOverwritePolicy::Error => {
                    return Err(FsInjectorError::Invalid("Destination entry already exists"));
                }
                StdOverwritePolicy::Skip => {
                    self.skipped_count += 1;
                    return Ok(());
                }
                StdOverwritePolicy::Replace => {}
            }
        }

        Ok(())
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if self.dir_stack.len() > 1 {
            self.dir_stack.pop();
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        Ok(())
    }
}
