// SPDX-License-Identifier: MIT

//! Path utilities for portable filesystem parsing and injection.
//!
//! This module provides helpers to normalize, split, join, and process filesystem paths.
//! All functions are no_std + alloc safe.
//!
//! Paths are always converted to `/`-separated form internally.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};

/// Normalize a full path to a relative logical path:
/// - strip a base prefix (ex: layout base dir)
/// - remove leading slashes
/// - unify separators to `/`
/// - returns an alloc::String usable in FsNode::name or for FS injection
pub fn normalize_relative_path(full_path: &str, base_prefix: &str) -> String {
    let relative = full_path.strip_prefix(base_prefix).unwrap_or(full_path);

    // Clean leading slashes
    let relative = relative.trim_start_matches(&['\\', '/'][..]);

    let mut out = String::new();
    for c in relative.chars() {
        if c == '\\' {
            out.push('/');
        } else {
            out.push(c);
        }
    }

    out
}

/// Convert a Path as &str into a unified path with `/` separators
pub fn path_to_unified_str(path_str: &str) -> String {
    let mut out = String::new();
    for c in path_str.chars() {
        if c == '\\' {
            out.push('/');
        } else {
            out.push(c);
        }
    }
    out
}

/// Join two path components with `/`, ensuring no duplicate slash
pub fn join_paths(base: &str, part: &str) -> String {
    let mut out = String::new();
    out.push_str(base.trim_end_matches('/'));
    out.push('/');
    out.push_str(part.trim_start_matches('/'));
    out
}

/// Splits a path into its components, using `/` as separator.
///
/// Returns a Vec of non-empty components.
pub fn split_path(path: &str) -> Vec<&str> {
    let mut parts = vec![];

    for part in path.split('/') {
        if !part.is_empty() {
            parts.push(part);
        }
    }

    parts
}

/// Cleans and normalizes a path string:
/// - strips Windows extended path prefix (`\\?\` if present)
/// - converts backslashes to `/`
///
/// Returns a normalized path string.
pub fn clean_and_normalize_path(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);

    let mut out = String::new();
    for c in path.chars() {
        if c == '\\' {
            out.push('/');
        } else {
            out.push(c);
        }
    }
    out
}

/// Returns `true` if the path ends with a wildcard component (`*`).
///
/// Example: `path/to/dir/*` → true.
pub fn is_wildcard(path: &str) -> bool {
    path.rsplit_once(['/', '\\'])
        .is_some_and(|(_, suffix)| suffix == "*")
}

/// Removes the wildcard suffix (`*`) from the path, if present.
///
/// Example: `path/to/dir/*` → `path/to/dir`.
pub fn strip_wildcard(path: &str) -> &str {
    if let Some((base, suffix)) = path.rsplit_once(['/', '\\'])
        && suffix == "*"
    {
        return base;
    }
    path
}

/// Extracts the last component of the path (file or directory name).
///
/// Example: `path/to/file.txt` → `file.txt`.
pub fn extract_name_from_path(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_utils_basic() {
        let p = "C:\\test\\dir\\file.txt";
        let unified = path_to_unified_str(p);
        assert_eq!(unified.as_str(), "C:/test/dir/file.txt");

        let joined = join_paths("path/to", "dir/file.txt");
        assert_eq!(joined.as_str(), "path/to/dir/file.txt");

        let name = extract_name_from_path("path/to/dir/file.txt");
        assert_eq!(name, "file.txt");

        assert!(is_wildcard("dir/*"));
        assert_eq!(strip_wildcard("dir/*"), "dir");

        let split = split_path("path/to/dir/file.txt");
        assert_eq!(split.as_slice(), ["path", "to", "dir", "file.txt"]);

        let clean = clean_and_normalize_path(r"\\?\C:\my\path\file.txt");
        assert_eq!(clean.as_str(), "C:/my/path/file.txt");

        let norm = normalize_relative_path("/base/dir/file.txt", "/base");
        assert_eq!(norm.as_str(), "dir/file.txt");
    }
}
