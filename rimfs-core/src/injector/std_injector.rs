// SPDX-License-Identifier: MIT

#[cfg(feature = "std")]
use std::{
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[cfg(feature = "std")]
use crate::{
    errors::{FsInjectorError, FsInjectorResult},
    injector::FsTreeInjector,
    resolver::attr::FileAttributes,
};
#[cfg(feature = "std")]
use rimio::RimRead;

/// Standard host filesystem injector implementing [FsTreeInjector] using the local filesystem.
///
/// This injector safely extracts or streams trees directly into a host destination directory.
/// It strictly confines writes to the configured root directory, preventing any path traversal or root escape.
///
/// This implementation is only available when the `std` feature is enabled.
#[cfg(feature = "std")]
/// Policy for handling existing files and symlinks during host injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StdOverwritePolicy {
    /// Overwrite / replace existing destination files.
    #[default]
    Replace,
    /// Abort with an error if the destination file already exists.
    Error,
    /// Skip writing if the destination file already exists.
    Skip,
}

#[cfg(feature = "std")]
#[derive(Debug, Clone)]
pub struct StdInjector {
    root_path: PathBuf,
    dir_stack: Vec<PathBuf>,
    overwrite_policy: StdOverwritePolicy,
}

#[cfg(feature = "std")]
impl StdInjector {
    /// Creates a new [StdInjector] targeting the given root directory.
    ///
    /// The root directory will be created if it does not already exist, and canonicalized to guarantee confinement.
    pub fn new(root_path: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root_path.as_ref();
        fs::create_dir_all(root)?;
        let canonical_root = root.canonicalize()?;
        Ok(Self {
            root_path: canonical_root.clone(),
            dir_stack: vec![canonical_root],
            overwrite_policy: StdOverwritePolicy::default(),
        })
    }

    /// Configures the overwrite policy.
    pub fn with_overwrite_policy(mut self, policy: StdOverwritePolicy) -> Self {
        self.overwrite_policy = policy;
        self
    }

    /// Sets the overwrite policy.
    pub fn set_overwrite_policy(&mut self, policy: StdOverwritePolicy) {
        self.overwrite_policy = policy;
    }

    /// Returns the current overwrite policy.
    pub fn overwrite_policy(&self) -> StdOverwritePolicy {
        self.overwrite_policy
    }

    /// Returns the canonical root path this injector is confined to.
    #[inline]
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    /// Returns the current directory path on top of the directory context stack.
    #[inline]
    pub fn current_dir(&self) -> &Path {
        self.dir_stack.last().unwrap_or(&self.root_path)
    }

    /// Resolves and validates a child entry name inside the current directory context,
    /// enforcing strict confinement to prevent path traversal or escape.
    fn resolve_child_path(&self, name: &str) -> FsInjectorResult<PathBuf> {
        if name.is_empty() {
            return Err(FsInjectorError::Invalid("Entry name cannot be empty"));
        }

        // Reject directory separators and NUL bytes in individual entry names
        if name.contains('/') || name.contains('\\') || name.contains('\0') {
            return Err(FsInjectorError::Invalid(
                "Entry name cannot contain path separators or NUL",
            ));
        }

        // On Windows and general: reject drive letters / alternate data streams
        if name.contains(':') {
            return Err(FsInjectorError::Invalid(
                "Entry name cannot contain colon or stream prefixes",
            ));
        }

        let current_dir = self
            .dir_stack
            .last()
            .ok_or(FsInjectorError::Invalid("Directory stack is empty"))?;

        // Inspect path component structure
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

        // Confinement check 1: current directory must be rooted within root_path
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

        // Confinement check 2: if target already exists and resolves outside root, reject immediately
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

#[cfg(feature = "std")]
impl FsTreeInjector<()> for StdInjector {
    fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
        self.dir_stack.clear();
        self.dir_stack.push(self.root_path.clone());
        Ok(())
    }

    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        let target = self.resolve_child_path(name)?;

        match fs::symlink_metadata(&target) {
            Ok(meta) => {
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
                // Verify existing directory canonicalizes inside root
                let canon = target.canonicalize()?;
                if !canon.starts_with(&self.root_path) {
                    return Err(FsInjectorError::Invalid(
                        "Root escape detected: directory resolves outside root",
                    ));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir_all(&target)?;
            }
            Err(e) => return Err(FsInjectorError::IO(rimio::errors::RimIOError::from(e))),
        }

        #[cfg(unix)]
        if let Some(mode) = attr.mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }

        #[cfg(not(unix))]
        let _ = attr;

        self.dir_stack.push(target);
        Ok(())
    }

    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let target = self.resolve_child_path(name)?;

        match fs::symlink_metadata(&target) {
            Ok(meta) => {
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
                        return Ok(());
                    }
                    StdOverwritePolicy::Replace => {
                        // Proceed to truncate and rewrite with File::create
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File does not exist yet; will be created
            }
            Err(e) => return Err(FsInjectorError::IO(rimio::errors::RimIOError::from(e))),
        }

        let mut file = fs::File::create(&target)?;

        // Stream source payload directly in bounded chunks
        let mut buf = [0u8; 64 * 1024];
        let mut offset = 0u64;
        let mut remaining = size;
        while remaining > 0 {
            let to_read = remaining.min(buf.len() as u64) as usize;
            source.read_at(offset, &mut buf[..to_read])?;
            file.write_all(&buf[..to_read])?;
            offset += to_read as u64;
            remaining -= to_read as u64;
        }
        file.flush()?;

        #[cfg(unix)]
        if let Some(mode) = attr.mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
        }

        if attr.read_only
            && let Ok(meta) = file.metadata()
        {
            let mut perms = meta.permissions();
            perms.set_readonly(true);
            let _ = fs::set_permissions(&target, perms);
        }

        if let Some(modified) = attr.modified {
            let ts = modified.unix_timestamp();
            if ts >= 0 {
                let st = std::time::UNIX_EPOCH + std::time::Duration::from_secs(ts as u64);
                let _ = file.set_modified(st);
            }
        }

        Ok(())
    }

    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        _attr: &FileAttributes,
    ) -> FsInjectorResult {
        let link_path = self.resolve_child_path(name)?;

        if let Ok(meta) = fs::symlink_metadata(&link_path) {
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
                    return Ok(());
                }
                StdOverwritePolicy::Replace => {
                    fs::remove_file(&link_path)?;
                }
            }
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, &link_path)?;
            Ok(())
        }
        #[cfg(windows)]
        {
            let is_dir =
                target.ends_with('/') || target.ends_with('\\') || Path::new(target).is_dir();
            let res = if is_dir {
                std::os::windows::fs::symlink_dir(target, &link_path)
            } else {
                std::os::windows::fs::symlink_file(target, &link_path)
            };
            match res {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    Err(FsInjectorError::Unsupported(
                        "Windows symlink creation requires elevated privileges or Developer Mode",
                    ))
                }
                Err(e) => Err(FsInjectorError::IO(rimio::errors::RimIOError::from(e))),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link_path);
            Err(FsInjectorError::Unsupported(
                "Symlinks not supported on this host platform",
            ))
        }
    }

    fn flush_current(&mut self) -> FsInjectorResult {
        if self.dir_stack.len() > 1 {
            self.dir_stack.pop();
        }
        Ok(())
    }

    fn flush(&mut self) -> FsInjectorResult {
        self.dir_stack.truncate(1);
        Ok(())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::StdResolver;
    use crate::resolver::{FileAttributes, FsTreeResolver};
    use rimio::VecRimIO;

    #[test]
    fn test_std_injector_basic_and_confinement() {
        let temp_dir =
            std::env::temp_dir().join(format!("rim_std_injector_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let mut injector = StdInjector::new(&temp_dir).expect("Failed to create StdInjector");

        // Confinement tests: path traversal attempts must be rejected
        assert!(injector.resolve_child_path("..").is_err());
        assert!(injector.resolve_child_path(".").is_err());
        assert!(injector.resolve_child_path("foo/bar").is_err());
        assert!(injector.resolve_child_path("foo\\bar").is_err());
        assert!(injector.resolve_child_path("C:escape").is_err());
        assert!(injector.resolve_child_path("").is_err());

        // Basic directory and file creation
        let dir_attr = FileAttributes::new_dir();
        injector.write_dir("sub", &dir_attr).unwrap();

        let file_content = b"hello streaming std injector!";
        let mut source = VecRimIO::new(file_content.to_vec());
        let file_attr = FileAttributes::new_file();
        injector
            .write_file(
                "test.txt",
                &mut source,
                file_content.len() as u64,
                &file_attr,
            )
            .unwrap();

        injector.flush_current().unwrap();
        injector.flush().unwrap();

        // Verify with StdResolver
        let mut resolver = StdResolver::new();
        let sub_path = temp_dir.join("sub").to_str().unwrap().to_string();
        let entries = resolver.read_dir(&sub_path).unwrap();
        assert_eq!(entries, vec!["test.txt"]);

        let test_file_path = temp_dir
            .join("sub")
            .join("test.txt")
            .to_str()
            .unwrap()
            .to_string();
        let read_back = resolver.read_file(&test_file_path).unwrap();
        assert_eq!(read_back, file_content);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_std_injector_symlink_confinement_and_type_mismatches() {
        let temp_dir =
            std::env::temp_dir().join(format!("rim_std_injector_sym_{}", std::process::id()));
        let outside_dir =
            std::env::temp_dir().join(format!("rim_std_outside_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::remove_dir_all(&outside_dir);
        fs::create_dir_all(&outside_dir).unwrap();

        let mut injector = StdInjector::new(&temp_dir).expect("Failed to create StdInjector");

        // 1. Type mismatch tests
        let dir_attr = FileAttributes::new_dir();
        let file_attr = FileAttributes::new_file();
        injector.write_dir("real_dir", &dir_attr).unwrap();
        injector.flush_current().unwrap();

        // Attempting to write regular file over existing directory must fail
        let mut src = VecRimIO::new(b"data".to_vec());
        let res = injector.write_file("real_dir", &mut src, 4, &file_attr);
        assert!(
            matches!(res, Err(FsInjectorError::Invalid(msg)) if msg.contains("Cannot replace directory with regular file")),
            "Expected directory-to-file mismatch error, got: {:?}",
            res
        );

        // Create a regular file
        let mut src = VecRimIO::new(b"hello".to_vec());
        injector
            .write_file("real_file.txt", &mut src, 5, &file_attr)
            .unwrap();

        // Attempting to create directory over existing regular file must fail
        let res = injector.write_dir("real_file.txt", &dir_attr);
        assert!(
            matches!(res, Err(FsInjectorError::Invalid(msg)) if msg.contains("Cannot replace regular file with directory")),
            "Expected file-to-directory mismatch error, got: {:?}",
            res
        );

        // 2. Existing symlink pointing outside root
        // Try creating a junction/symlink in temp_dir pointing to outside_dir
        #[cfg(windows)]
        let link_created = {
            let link_path = temp_dir.join("escape_link");
            let cmd_status = std::process::Command::new("cmd")
                .args(["/c", "mklink", "/J"])
                .arg(&link_path)
                .arg(&outside_dir)
                .output();
            cmd_status.is_ok_and(|o| o.status.success())
        };
        #[cfg(unix)]
        let link_created = {
            let link_path = temp_dir.join("escape_link");
            std::os::unix::fs::symlink(&outside_dir, &link_path).is_ok()
        };
        #[cfg(not(any(windows, unix)))]
        let link_created = false;

        if link_created {
            // Attempting to traverse into escape_link via write_dir must fail
            let res = injector.write_dir("escape_link", &dir_attr);
            assert!(
                res.is_err(),
                "Expected write_dir into existing symlink to fail, got: {:?}",
                res
            );

            // Attempting to write_file through escape_link must fail
            let mut src = VecRimIO::new(b"exploit".to_vec());
            let res = injector.write_file("escape_link", &mut src, 7, &file_attr);
            assert!(
                res.is_err(),
                "Expected write_file to existing symlink to fail, got: {:?}",
                res
            );

            // Verify no file was created in outside_dir
            let outside_entries: Vec<_> = fs::read_dir(&outside_dir).unwrap().collect();
            assert!(
                outside_entries.is_empty(),
                "Outside directory was modified!"
            );
        }

        // Cleanup
        #[cfg(windows)]
        {
            let link_path = temp_dir.join("escape_link");
            let _ = std::process::Command::new("cmd")
                .args(["/c", "rmdir"])
                .arg(&link_path)
                .output();
        }
        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::remove_dir_all(&outside_dir);
    }

    #[test]
    fn test_std_injector_asymmetric_replace_and_policies() {
        let temp_dir =
            std::env::temp_dir().join(format!("rim_std_injector_rep_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        let mut injector = StdInjector::new(&temp_dir).expect("Failed to create StdInjector");

        // 1. Initial 10 MiB payload (known pattern)
        let large_size = 10 * 1024 * 1024usize;
        let large_payload = vec![0xAAu8; large_size];
        let mut src_large = VecRimIO::new(large_payload.clone());
        let file_attr = FileAttributes::new_file();

        injector
            .write_file("data.bin", &mut src_large, large_size as u64, &file_attr)
            .unwrap();

        let file_path = temp_dir.join("data.bin");
        assert_eq!(fs::metadata(&file_path).unwrap().len(), large_size as u64);

        // 2. Replace with 1 KiB payload (hash B)
        let small_size = 1024usize;
        let small_payload = vec![0xBBu8; small_size];
        let mut src_small = VecRimIO::new(small_payload.clone());

        injector
            .write_file("data.bin", &mut src_small, small_size as u64, &file_attr)
            .unwrap();

        // Verify: size is exactly 1 KiB, content is B, no stale data
        let replaced_content = fs::read(&file_path).unwrap();
        assert_eq!(replaced_content.len(), small_size);
        assert_eq!(replaced_content, small_payload);

        // 3. Replace back with 10 MiB payload
        let mut src_large2 = VecRimIO::new(large_payload.clone());
        injector
            .write_file("data.bin", &mut src_large2, large_size as u64, &file_attr)
            .unwrap();
        assert_eq!(fs::metadata(&file_path).unwrap().len(), large_size as u64);

        // 4. Test Error policy
        injector.set_overwrite_policy(StdOverwritePolicy::Error);
        let mut src_err = VecRimIO::new(vec![0xCCu8; 100]);
        let res = injector.write_file("data.bin", &mut src_err, 100, &file_attr);
        assert!(res.is_err(), "Error policy should reject existing file");

        // 5. Test Skip policy
        injector.set_overwrite_policy(StdOverwritePolicy::Skip);
        let mut src_skip = VecRimIO::new(vec![0xDDu8; 100]);
        let res = injector.write_file("data.bin", &mut src_skip, 100, &file_attr);
        assert!(res.is_ok(), "Skip policy should succeed without error");
        // File content should remain unchanged (10 MiB, not 100 bytes)
        assert_eq!(fs::metadata(&file_path).unwrap().len(), large_size as u64);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
