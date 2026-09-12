// SPDX-License-Identifier: MIT

//! Logical directory tree injection traits and streaming abstractions.

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

/// Persistent lifecycle state for an injector. A failure is terminal for this
/// instance; reopening does not imply that partial disk changes were repaired.
#[derive(Default)]
pub struct FsInjectorState {
    failed: bool,
}

/// Backend operations for injectors using the shared failure lifecycle.
/// Implementations provide disk operations and storage for the state; callers use
/// `FsTreeInjector`, whose blanket implementation guards every primitive operation.
/// Backend hooks must not be called directly by application code.
pub trait FsTreeInjectorBackend {
    type Handle: FsHandle;
    fn injector_state(&mut self) -> &mut FsInjectorState;
    fn set_root_context_inner(&mut self, attr: &FileAttributes) -> FsInjectorResult;
    fn write_dir_inner(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult;
    fn write_file_inner(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult;
    fn write_symlink_inner(
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
    fn flush_current_inner(&mut self) -> FsInjectorResult {
        Ok(())
    }
    fn flush_inner(&mut self) -> FsInjectorResult {
        Ok(())
    }

    /// Guard additional engine-specific mutation APIs with the same state.
    fn mutation<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> FsInjectorResult<T>,
    ) -> FsInjectorResult<T>
    where
        Self: Sized,
    {
        if self.injector_state().failed {
            return Err(FsInjectorError::Invalid(
                "Injector failed; discard and inspect volume before reopening",
            ));
        }
        let result = operation(self);
        if result.is_err() {
            self.injector_state().failed = true;
        }
        result
    }
}

impl<T: FsTreeInjectorBackend> FsTreeInjector<T::Handle> for T {
    fn check_active(&mut self) -> FsInjectorResult {
        if self.injector_state().failed {
            Err(FsInjectorError::Invalid(
                "Injector failed; discard and inspect volume before reopening",
            ))
        } else {
            Ok(())
        }
    }
    fn operation_failed(&mut self) {
        self.injector_state().failed = true;
    }
    fn set_root_context(&mut self, attr: &FileAttributes) -> FsInjectorResult {
        self.mutation(|this| this.set_root_context_inner(attr))
    }
    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult {
        self.mutation(|this| this.write_dir_inner(name, attr))
    }
    fn write_file(
        &mut self,
        name: &str,
        source: &mut dyn RimRead,
        size: u64,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        self.mutation(|this| this.write_file_inner(name, source, size, attr))
    }
    fn write_symlink(
        &mut self,
        name: &str,
        target: &str,
        attr: &FileAttributes,
    ) -> FsInjectorResult {
        self.mutation(|this| this.write_symlink_inner(name, target, attr))
    }
    fn flush_current(&mut self) -> FsInjectorResult {
        self.mutation(Self::flush_current_inner)
    }
    fn flush(&mut self) -> FsInjectorResult {
        self.mutation(Self::flush_inner)
    }
}

/// High-level injector for filesystem trees (files and directories).
///
/// Previously `FsNodeInjector`.
pub trait FsTreeInjector<Handle: FsHandle> {
    /// Lifecycle hooks used by traversal helpers, including source/resolver errors.
    /// Legacy direct implementations retain their existing failure policy.
    #[doc(hidden)]
    fn check_active(&mut self) -> FsInjectorResult {
        Ok(())
    }
    #[doc(hidden)]
    fn operation_failed(&mut self) {}

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
        self.check_active()?;
        let result = (|| {
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
        })();
        if result.is_err() {
            self.operation_failed();
        }
        result
    }

    /// Full-tree injection helper.
    #[must_use = "injection result must be checked for errors"]
    fn inject_tree(&mut self, node: &mut FsNode<'_>) -> FsInjectorResult {
        self.check_active()?;
        let result = (|| {
            self.set_root_context(node.attr())?;
            self.inject_node(node, true)?;
            self.flush()?;
            Ok(())
        })();
        if result.is_err() {
            self.operation_failed();
        }
        result
    }

    /// Single-entry injection helper (no recursion).
    #[must_use = "injection result must be checked for errors"]
    fn inject_entry(&mut self, node: &mut FsNode<'_>) -> FsInjectorResult {
        self.check_active()?;
        let result = (|| {
            self.set_root_context(node.attr())?;
            self.inject_node(node, false)?;
            self.flush()?;
            Ok(())
        })();
        if result.is_err() {
            self.operation_failed();
        }
        result
    }

    /// Injects a resolver tree without first materializing file payloads into `FsNode`s.
    #[must_use = "injection result must be checked for errors"]
    fn inject_tree_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        self.check_active()?;
        let result = (|| {
            let root_attr = if is_wildcard(path) {
                FileAttributes::new_dir()
            } else {
                resolver.read_attributes(path)?
            };
            self.set_root_context(&root_attr)?;
            let counts = self.inject_from_resolver(resolver, path, true)?;
            self.flush()?;
            Ok(counts)
        })();
        if result.is_err() {
            self.operation_failed();
        }
        result
    }

    /// Injects one resolver entry without recursing into subdirectories.
    #[must_use = "injection result must be checked for errors"]
    fn inject_entry_from_resolver(
        &mut self,
        resolver: &mut dyn FsTreeResolver,
        path: &str,
    ) -> FsInjectorResult<FsNodeCounts> {
        self.check_active()?;
        let result = (|| {
            let root_attr = if is_wildcard(path) {
                FileAttributes::new_dir()
            } else {
                resolver.read_attributes(path)?
            };
            self.set_root_context(&root_attr)?;
            let counts = self.inject_from_resolver(resolver, path, false)?;
            self.flush()?;
            Ok(counts)
        })();
        if result.is_err() {
            self.operation_failed();
        }
        result
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
        self.check_active()?;
        let result = (|| {
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
        })();
        if result.is_err() {
            self.operation_failed();
        }
        result
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
    use alloc::boxed::Box;
    #[derive(Clone, Copy)]
    struct Handle;
    impl FsHandle for Handle {}
    #[derive(Default)]
    struct Backend {
        state: FsInjectorState,
        calls: usize,
    }
    impl FsTreeInjectorBackend for Backend {
        type Handle = Handle;
        fn injector_state(&mut self) -> &mut FsInjectorState {
            &mut self.state
        }
        fn set_root_context_inner(&mut self, _: &FileAttributes) -> FsInjectorResult {
            self.calls += 1;
            Ok(())
        }
        fn write_dir_inner(&mut self, _: &str, _: &FileAttributes) -> FsInjectorResult {
            self.calls += 1;
            Ok(())
        }
        fn write_file_inner(
            &mut self,
            _: &str,
            _: &mut dyn RimRead,
            _: u64,
            _: &FileAttributes,
        ) -> FsInjectorResult {
            self.calls += 1;
            Ok(())
        }
        fn flush_inner(&mut self) -> FsInjectorResult {
            self.calls += 1;
            Ok(())
        }
    }
    struct FailedSource;
    impl RimRead for FailedSource {
        fn read_at(&mut self, _: u64, _: &mut [u8]) -> rimio::RimIOResult {
            Err(rimio::RimIOError::Other("source failed"))
        }
        fn total_size(&mut self) -> rimio::RimIOResult<u64> {
            Err(rimio::RimIOError::Other("source failed"))
        }
    }
    #[test]
    fn source_failure_invalidates_shared_lifecycle_before_backend_file_call() {
        let mut backend = Backend::default();
        let mut node = FsNode::new_file_from_source(
            "file",
            Box::new(FailedSource),
            FileAttributes::new_file(),
        );
        assert!(backend.inject_tree(&mut node).is_err());
        assert_eq!(backend.calls, 1); // root setup only
        assert!(backend.flush().is_err());
        assert!(
            backend
                .set_root_context(&FileAttributes::new_dir())
                .is_err()
        );
        assert!(
            backend
                .write_dir("later", &FileAttributes::new_dir())
                .is_err()
        );
        assert!(backend.inject_node(&mut node, true).is_err());
        assert_eq!(backend.calls, 1);
    }
}
