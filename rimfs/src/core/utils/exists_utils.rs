// SPDX-License-Identifier: MIT

//! Filesystem test utilities.
//!
//! This module provides common helper functions for verifying
//! the structure and content of a parsed filesystem tree (`FsParser`).
//!
//! These functions are typically used in tests to validate
//! that the content injected or parsed matches expectations.

use crate::core::{FsResult, resolver::FsResolver};

/// Checks if a file exists at the given path.
///
/// Returns `Ok(())` if the path exists and is a file, or an `FsError` otherwise.
pub fn check_file_exists<P: FsResolver>(parser: &mut P, path: &str) -> FsResult {
    let node = parser.parse_path(path)?;
    if node.is_file() {
        Ok(())
    } else {
        Err("Expected file at path {path}, found dir".into())
    }
}

/// Checks if a directory exists at the given path.
///
/// Returns `Ok(())` if the path exists and is a directory, or an `FsError` otherwise.
pub fn check_dir_exists<P: FsResolver>(parser: &mut P, path: &str) -> FsResult {
    let node = parser.parse_path(path)?;
    if node.is_dir() {
        Ok(())
    } else {
        Err("Expected dir at path {path}, found file".into())
    }
}

/// Checks if the content of a file at the given path matches `expected_content`.
///
/// Returns `Ok(())` if the content matches exactly, or an `FsError` otherwise.
pub fn check_file_content<P: FsResolver>(
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
    use crate::core::StdResolver;

    use super::*;

    #[test]
    fn test_check_file_and_dir_exists() {
        let mut parser = StdResolver::new();

        // Should succeed if run in a normal project root.
        check_dir_exists(&mut parser, "src").expect("Expected 'src' to be a directory");

        // This file (fs_test_utils.rs) must exist:
        check_file_exists(&mut parser, "src/core/utils/exists_utils.rs")
            .expect("Expected fs_utils.rs to exist");
    }
}
