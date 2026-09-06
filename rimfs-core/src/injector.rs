// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

pub use crate::errors::{FsInjectorError, FsInjectorResult};
pub use crate::resolver::{FsNode, FsTreeResolver};

#[cfg(feature = "std")]
pub mod std_injector;
#[cfg(feature = "std")]
pub use std_injector::{StdInjector, StdOverwritePolicy};

use crate::{
    allocator::FsHandle,
    resolver::attr::{FileAttributes, NodeKind},
    resolver::node::FsNodeCounts,
    utils::path_utils::{extract_name_from_path, is_wildcard, join_paths, strip_wildcard},
};

pub struct FsContext<Handle: FsHandle, B = Vec<u8>> {
    pub handle: Handle,
    pub buf: B,
}

impl<Handle: FsHandle, B> FsContext<Handle, B> {
    pub fn new(handle: Handle, buf: B) -> Self {
        Self { handle, buf }
    }
}

use rimio::prelude::RimRead;

/// Low-level injector for filesystem units/resources.
///
/// Implementations handle writing specific structures (inodes, clusters, records)
/// to the underlying storage.
pub trait FsInjector<Handle: FsHandle> {
    /// Write data to the location identified by `handle`.
    fn write_unit(&mut self, handle: Handle, data: &[u8]) -> FsInjectorResult;
}

/// High-level injector for filesystem trees (files and directories).
///
/// Previously `FsNodeInjector`.
pub trait FsTreeInjector<Handle: FsHandle> {
    /// Create a new directory under the current directory.
    #[must_use = "injection result must be checked for errors"]
    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult;

    /// Create a file under the current directory.
    ///
    /// Reads `size` bytes from `source` and writes them to the new file.
    /// `size` must match the available data in `source`.
    #[must_use = "injection result must be checked for errors"]
    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult;

    /// Create a symbolic link under the current directory pointing to `target`.
    #[must_use = "injection result must be checked for errors"]
    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        let _ = (name, target, attr);
        Err(FsInjectorError::Unsupported(
            "Symlinks are not supported on this filesystem",
        ))
    }

    /// Initialize the root directory context.
    fn set_root_context(&mut self, attr: &FileAttributes) -> FsInjectorResult;

    /// Recursive helper: directories are linked before their children are written.
    fn inject_node(&mut self, node: &mut FsNode<'_>, recurse: bool) -> FsInjectorResult {
        match node {
            FsNode::File { name, source, attr } => {
                let size = source.total_size().map_err(FsInjectorError::IO)?;
                self.write_file(name, source.as_mut(), size, attr)?;
            }
            FsNode::Dir {
                name,
                children,
                attr,
            } => {
                if !name.is_empty() {
                    self.write_dir(name, attr)?;
                }
                if recurse {
                    for child in children {
                        self.inject_node(child, recurse)?;
                    }
                }
                self.flush_current()?;
            }
            FsNode::Symlink { name, target, attr } => {
                self.write_symlink(name, target, attr)?;
            }
            FsNode::Container { children, .. } => {
                for child in children {
                    self.inject_node(child, recurse)?;
                }
                self.flush_current()?;
            }
        }
        Ok(())
    }

    /// Full-tree injection helper.
    #[must_use = "injection result must be checked for errors"]
    fn inject_tree(&mut self, node: &mut FsNode<'_>) -> FsInjectorResult {
        self.set_root_context(node.attr())?;
        self.inject_node(node, true)?;
        self.flush()?;
        Ok(())
    }

    /// Single-entry injection helper (no recursion).
    #[must_use = "injection result must be checked for errors"]
    fn inject_entry(&mut self, node: &mut FsNode<'_>) -> FsInjectorResult {
        self.set_root_context(node.attr())?;
        self.inject_node(node, false)?;
        self.flush()?;
        Ok(())
    }

    /// Injects a resolver tree without first materializing file payloads into `FsNode`s.
    #[must_use = "injection result must be checked for errors"]
    fn inject_tree_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        let root_attr = if is_wildcard(path) {
            FileAttributes::new_dir()
        } else {
            resolver.read_attributes(path)?
        };
        self.set_root_context(&root_attr)?;
        let counts = self.inject_from_resolver(resolver, path, true)?;
        self.flush()?;
        Ok(counts)
    }

    /// Injects one resolver entry without recursing into subdirectories.
    #[must_use = "injection result must be checked for errors"]
    fn inject_entry_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        let root_attr = if is_wildcard(path) {
            FileAttributes::new_dir()
        } else {
            resolver.read_attributes(path)?
        };
        self.set_root_context(&root_attr)?;
        let counts = self.inject_from_resolver(resolver, path, false)?;
        self.flush()?;
        Ok(counts)
    }

    /// Recursive resolver injection helper.
    ///
    /// At most one source file stream is alive at a time.
    fn inject_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
        recurse: bool,
    ) -> FsInjectorResult<FsNodeCounts> {
        if is_wildcard(path) {
            let base_path = strip_wildcard(path);
            let mut counts = FsNodeCounts::default();
            for entry in resolver.read_dir(base_path)? {
                let entry_path = join_paths(base_path, &entry);
                let child_counts = self.inject_from_resolver(resolver, &entry_path, recurse)?;
                counts.dirs += child_counts.dirs;
                counts.files += child_counts.files;
                counts.symlinks += child_counts.symlinks;
                counts.bytes += child_counts.bytes;
            }
            self.flush_current()?;
            return Ok(counts);
        }

        let attr = resolver.read_attributes(path)?;
        let name = extract_name_from_path(path);
        let mut counts = FsNodeCounts::default();

        match attr.kind {
            NodeKind::Directory => {
                counts.dirs += 1;
                if !name.is_empty() {
                    self.write_dir(name, &attr)?;
                }
                if recurse {
                    for entry in resolver.read_dir(path)? {
                        let entry_path = join_paths(path, &entry);
                        let child_counts =
                            self.inject_from_resolver(resolver, &entry_path, recurse)?;
                        counts.dirs += child_counts.dirs;
                        counts.files += child_counts.files;
                        counts.symlinks += child_counts.symlinks;
                        counts.bytes += child_counts.bytes;
                    }
                }
                self.flush_current()?;
            }
            NodeKind::Regular => {
                let mut source = resolver.open_file(path)?;
                let size = source.total_size().map_err(FsInjectorError::IO)?;
                self.write_file(name, source.as_mut(), size, &attr)?;
                counts.files += 1;
                counts.bytes += size;
            }
            NodeKind::Symlink => {
                let target = resolver.read_link(path)?;
                self.write_symlink(name, &target, &attr)?;
                counts.symlinks += 1;
            }
            _ => {
                return Err(FsInjectorError::Unsupported(
                    "Unsupported resolver entry kind",
                ));
            }
        }

        Ok(counts)
    }

    /// Write the current directory context to disk and pop it.
    fn flush_current(&mut self) -> FsInjectorResult {
        Ok(())
    }

    /// Drain and write all remaining directory contexts.
    fn flush(&mut self) -> FsInjectorResult {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::FsResolverResult;
    use crate::resolver::FileAttributes;
    use alloc::{boxed::Box, string::String, vec::Vec};
    use core::cell::Cell;
    use rimio::{RimIOError, RimIOResult};
    use std::rc::Rc;

    #[derive(Clone, Copy)]
    struct TestHandle;

    impl FsHandle for TestHandle {}

    struct GeneratedRead {
        size: u64,
        max_read_len: usize,
        active: Rc<Cell<usize>>,
    }

    impl GeneratedRead {
        fn new(
            size: u64,
            max_read_len: usize,
            active: Rc<Cell<usize>>,
            max_active: Rc<Cell<usize>>,
        ) -> Self {
            let current = active.get() + 1;
            active.set(current);
            max_active.set(max_active.get().max(current));
            Self {
                size,
                max_read_len,
                active,
            }
        }
    }

    impl Drop for GeneratedRead {
        fn drop(&mut self) {
            self.active.set(self.active.get() - 1);
        }
    }

    impl RimRead for GeneratedRead {
        fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> RimIOResult {
            if buf.len() > self.max_read_len {
                return Err(RimIOError::Other("read buffer too large"));
            }
            let end = offset
                .checked_add(buf.len() as u64)
                .ok_or(RimIOError::OutOfBounds)?;
            if end > self.size {
                return Err(RimIOError::OutOfBounds);
            }
            buf.fill((offset % 251) as u8);
            Ok(())
        }

        fn total_size(&mut self) -> RimIOResult<u64> {
            Ok(self.size)
        }
    }

    struct TestResolver {
        active: Rc<Cell<usize>>,
        max_active: Rc<Cell<usize>>,
    }

    impl TestResolver {
        fn new(active: Rc<Cell<usize>>, max_active: Rc<Cell<usize>>) -> Self {
            Self { active, max_active }
        }

        fn file_size(path: &str) -> Option<u64> {
            match path.trim_matches('/') {
                "empty.txt" => Some(0),
                "huge.bin" => Some(3 * 1024 * 1024 * 1024),
                "unicodé.txt" => Some(12),
                "nested/child.bin" => Some(4096),
                _ => None,
            }
        }
    }

    impl FsTreeResolver for TestResolver {
        fn read_attributes(&mut self, path: &str) -> FsResolverResult<FileAttributes> {
            let clean = path.trim_matches('/');
            if clean.is_empty() || clean == "nested" {
                return Ok(FileAttributes::new_dir());
            }
            if clean == "link" {
                return Ok(FileAttributes::new_symlink());
            }
            if Self::file_size(clean).is_some() {
                return Ok(FileAttributes::new_file());
            }
            Err(crate::errors::FsResolverError::NotFound)
        }

        fn read_dir(&mut self, path: &str) -> FsResolverResult<Vec<String>> {
            match path.trim_matches('/') {
                "" => Ok(alloc::vec![
                    "empty.txt".into(),
                    "huge.bin".into(),
                    "link".into(),
                    "nested".into(),
                    "unicodé.txt".into(),
                ]),
                "nested" => Ok(alloc::vec!["child.bin".into()]),
                _ => Err(crate::errors::FsResolverError::NotFound),
            }
        }

        fn open_file<'a>(&'a mut self, path: &str) -> FsResolverResult<Box<dyn RimRead + 'a>> {
            let size = Self::file_size(path.trim_matches('/'))
                .ok_or(crate::errors::FsResolverError::NotFound)?;
            Ok(Box::new(GeneratedRead::new(
                size,
                1024 * 1024,
                Rc::clone(&self.active),
                Rc::clone(&self.max_active),
            )))
        }

        fn read_link(&mut self, path: &str) -> FsResolverResult<String> {
            if path.trim_matches('/') == "link" {
                Ok("nested/child.bin".into())
            } else {
                Err(crate::errors::FsResolverError::NotFound)
            }
        }
    }

    #[derive(Default)]
    struct RecordingInjector {
        entries: Vec<String>,
    }

    impl FsTreeInjector<TestHandle> for RecordingInjector {
        fn write_dir(&mut self, name: &str, _attr: &FileAttributes) -> FsInjectorResult {
            self.entries.push(alloc::format!("dir:{name}"));
            Ok(())
        }

        fn write_file(
            &mut self,
            name: &str,
            source: &mut dyn RimRead,
            size: u64,
            _attr: &FileAttributes,
        ) -> FsInjectorResult {
            let mut remaining = size;
            let mut offset = 0;
            let mut buf = [0u8; 64 * 1024];
            while remaining > 0 {
                let n = remaining.min(buf.len() as u64) as usize;
                source.read_at(offset, &mut buf[..n])?;
                remaining -= n as u64;
                offset += n as u64;
            }
            self.entries.push(alloc::format!("file:{name}:{size}"));
            Ok(())
        }

        fn write_symlink(
            &mut self,
            name: &str,
            target: &str,
            _attr: &FileAttributes,
        ) -> FsInjectorResult {
            self.entries.push(alloc::format!("link:{name}->{target}"));
            Ok(())
        }

        fn set_root_context(&mut self, _attr: &FileAttributes) -> FsInjectorResult {
            self.entries.push("root".into());
            Ok(())
        }
    }

    #[test]
    fn inject_tree_from_resolver_streams_one_file_at_a_time() {
        let active = Rc::new(Cell::new(0));
        let max_active = Rc::new(Cell::new(0));
        let mut resolver = TestResolver::new(Rc::clone(&active), Rc::clone(&max_active));
        let mut injector = RecordingInjector::default();

        let counts = injector
            .inject_tree_from_resolver(&mut resolver, "/*")
            .expect("streaming resolver injection failed");

        assert_eq!(counts.dirs, 1);
        assert_eq!(counts.files, 4);
        assert_eq!(counts.symlinks, 1);
        assert_eq!(counts.bytes, 3 * 1024 * 1024 * 1024 + 4108);
        assert_eq!(active.get(), 0);
        assert_eq!(max_active.get(), 1);
        assert!(
            injector
                .entries
                .contains(&"file:huge.bin:3221225472".into())
        );
        assert!(injector.entries.contains(&"file:unicodé.txt:12".into()));
        assert!(
            injector
                .entries
                .contains(&"link:link->nested/child.bin".into())
        );
    }
}
