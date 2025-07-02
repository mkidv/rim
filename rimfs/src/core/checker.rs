// SPDX-License-Identifier: MIT

pub use crate::core::{FsCheckerError, FsCheckerResult};

/// Trait for verifying the integrity of a filesystem.
///
/// This trait is typically implemented for each specific filesystem (e.g. FAT32, EXT4)
/// to perform internal consistency checks (VBR validity, superblock, FAT chains, inodes, etc.).
pub trait FsChecker {
    /// Runs all available checks on the filesystem.
    ///
    /// Returns `Ok(())` if all checks pass, or an error detailing the first failure encountered.
    fn check_all(&mut self) -> FsCheckerResult;
}
