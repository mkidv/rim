// SPDX-License-Identifier: MIT

//! Filesystem test utilities.
//!
//! This module provides common helper functions for verifying
//! the structure and content of a parsed filesystem tree.

use crate::{FsResult, resolver::FsTreeResolver};

/// Checks if a file exists at the given path.
///
/// Returns `Ok(())` if the path exists and is a file, or an `FsError` otherwise.
pub fn check_file_exists<'a, P: FsTreeResolver<'a>>(parser: &mut P, path: &str) -> FsResult {
    let node = parser.resolve_entry(path)?;
    if node.is_file() {
        Ok(())
    } else {
        Err("Expected file at path, found dir".into())
    }
}

/// Checks if a directory exists at the given path.
///
/// Returns `Ok(())` if the path exists and is a directory, or an `FsError` otherwise.
pub fn check_dir_exists<'a, P: FsTreeResolver<'a>>(parser: &mut P, path: &str) -> FsResult {
    let node = parser.resolve_entry(path)?;
    if node.is_dir() {
        Ok(())
    } else {
        Err("Expected dir at path, found file".into())
    }
}

/// Checks if the content of a file at the given path matches `expected_content`.
///
/// Returns `Ok(())` if the content matches exactly, or an `FsError` otherwise.
pub fn check_file_content<'a, P: FsTreeResolver<'a>>(
    parser: &mut P,
    path: &str,
    expected_content: &[u8],
) -> FsResult {
    let content = parser.read_file(path)?;
    if content == expected_content {
        Ok(())
    } else {
        Err("File content mismatch".into())
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::StdResolver;

    #[test]
    fn test_check_file_and_dir_exists() {
        let mut parser = StdResolver::new();

        // Should succeed if run in a normal project root.
        check_dir_exists(&mut parser, "src").expect("Expected 'src' to be a directory");

        // This file must exist in rimfs-core after the flattening.
        check_file_exists(&mut parser, "src/utils/exists_utils.rs")
            .expect("Expected exists_utils.rs to exist");
    }
}
