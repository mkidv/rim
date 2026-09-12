// SPDX-License-Identifier: MIT

//! Generic in-memory path index for filesystem archive and image resolvers.
//!
//! Uses ordered maps for path lookup and immediate directory children.
//! Automatically tracks parent-to-children relations upon insertion without
//! splitting paths into temporary vectors. Owned listings clone names; borrowed listings do not.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::{String, ToString},
    vec::Vec,
};

#[cfg(feature = "std")]
use std::collections::{BTreeMap, BTreeSet};

use crate::utils::path_utils::normalize_fs_path;

/// An indexed tree mapping path strings to entries of type `E`, with cached child listings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathIndex<E> {
    entries: BTreeMap<String, E>,
    children: BTreeMap<String, BTreeSet<String>>,
}

impl<E> Default for PathIndex<E> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            children: BTreeMap::new(),
        }
    }
}

impl<E> PathIndex<E> {
    /// Creates a new empty `PathIndex`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts an entry associated with `path`.
    ///
    /// The path is normalized, and all ancestor directories are automatically
    /// recorded in the child index.
    pub fn insert(&mut self, path: &str, entry: E) {
        self.insert_with_kind(path, entry, path.ends_with('/'));
    }

    /// Insert an entry with an explicit directory kind, including empty directories.
    pub fn insert_with_kind(&mut self, path: &str, entry: E, is_dir: bool) {
        let norm_str = normalize_fs_path(path);
        let norm_path = norm_str.to_string();
        if is_dir {
            self.children.entry(norm_path.clone()).or_default();
        } else if self.children.get(norm_str).is_some_and(BTreeSet::is_empty) {
            self.children.remove(norm_str);
        }

        self.entries.insert(norm_path.clone(), entry);

        let mut parent = String::new();
        for component in norm_str.split('/').filter(|c| !c.is_empty()) {
            self.children
                .entry(parent.clone())
                .or_default()
                .insert(component.to_string());
            if !parent.is_empty() {
                parent.push('/');
            }
            parent.push_str(component);
        }
    }

    /// Looks up a reference to an entry by its path.
    #[inline]
    pub fn get(&self, path: &str) -> Option<&E> {
        let norm_cow = normalize_fs_path(path);
        let norm_str = norm_cow.trim_end_matches('/');
        self.entries.get(norm_str)
    }

    /// Looks up a mutable reference to an entry by its path.
    #[inline]
    pub fn get_mut(&mut self, path: &str) -> Option<&mut E> {
        let norm_cow = normalize_fs_path(path);
        let norm_str = norm_cow.trim_end_matches('/');
        self.entries.get_mut(norm_str)
    }

    /// Checks whether a path exists in the index (as an entry or as an intermediate directory).
    #[inline]
    pub fn contains_path(&self, path: &str) -> bool {
        let norm_cow = normalize_fs_path(path);
        let norm_str = norm_cow.trim_end_matches('/');
        norm_str.is_empty()
            || self.entries.contains_key(norm_str)
            || self.children.contains_key(norm_str)
    }

    /// Returns a list of immediate child names for the given directory path.
    #[inline]
    pub fn children(&self, path: &str) -> Option<Vec<String>> {
        self.children_iter(path)
            .map(|names| names.map(str::to_string).collect())
    }

    /// Borrow immediate child names without allocating a vector or cloning strings.
    pub fn children_iter(&self, path: &str) -> Option<impl Iterator<Item = &str>> {
        let path = normalize_fs_path(path);
        self.is_dir(path).then(|| {
            self.children
                .get(path)
                .into_iter()
                .flat_map(|names| names.iter().map(String::as_str))
        })
    }

    /// Checks whether the given path is a directory (root, explicit directory entry, or implicit parent).
    #[inline]
    pub fn is_dir(&self, path: &str) -> bool {
        let norm_cow = normalize_fs_path(path);
        let norm_str = norm_cow.trim_end_matches('/');
        norm_str.is_empty() || self.children.contains_key(norm_str)
    }

    /// Number of entries stored in the index.
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the index has no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clears all entries and directory relationships.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.children.clear();
    }

    /// Iterates over all `(&path, &entry)` pairs in alphabetical order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &E)> {
        self.entries.iter()
    }

    /// Returns a reference to the internal entries map.
    #[inline]
    pub fn entries(&self) -> &BTreeMap<String, E> {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn explicit_empty_directories_and_borrowed_children() {
        let mut index = PathIndex::new();
        assert_eq!(index.children_iter("/").unwrap().count(), 0);
        index.insert_with_kind("empty", 1, true);
        assert!(index.is_dir("empty"));
        assert_eq!(index.children_iter("empty").unwrap().count(), 0);
        index.insert_with_kind("empty", 2, false);
        assert!(!index.is_dir("empty"));
        index.insert("another/", 3);
        assert!(index.is_dir("another"));
        assert_eq!(
            index.children_iter("").unwrap().collect::<Vec<_>>(),
            ["another", "empty"]
        );
        index.clear();
        assert!(!index.contains_path("empty"));
    }

    #[test]
    fn test_path_index_basic() {
        let mut index: PathIndex<u32> = PathIndex::new();
        index.insert("a/b/c.txt", 42);
        index.insert("/a/b/d.txt/", 100);
        index.insert("root.txt", 1);

        assert_eq!(index.len(), 3);
        assert_eq!(index.get("a/b/c.txt"), Some(&42));
        assert_eq!(index.get("/a/b/c.txt"), Some(&42));
        assert_eq!(index.get("a/b/d.txt"), Some(&100));
        assert_eq!(index.get("root.txt"), Some(&1));
        assert_eq!(index.get("missing.txt"), None);

        assert!(index.contains_path(""));
        assert!(index.contains_path("/"));
        assert!(index.contains_path("a"));
        assert!(index.contains_path("a/b"));
        assert!(index.contains_path("a/b/c.txt"));
        assert!(!index.contains_path("a/c"));

        let root_children = index.children("").unwrap();
        assert_eq!(root_children, vec!["a".to_string(), "root.txt".to_string()]);

        let a_children = index.children("a").unwrap();
        assert_eq!(a_children, vec!["b".to_string()]);

        let ab_children = index.children("a/b").unwrap();
        assert_eq!(ab_children, vec!["c.txt".to_string(), "d.txt".to_string()]);
    }
}
