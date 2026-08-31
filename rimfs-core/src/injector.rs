// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

pub use crate::errors::{FsInjectorError, FsInjectorResult};
pub use crate::resolver::FsNode;

use crate::{allocator::FsHandle, resolver::attr::FileAttributes};

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
    ///
    /// `FsNode::Container` is treated as an anonymous root directory; other root
    /// nodes use their own attributes.
    fn set_root_context(&mut self, node: &FsNode<'_>) -> FsInjectorResult;

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
        self.set_root_context(node)?;
        self.inject_node(node, true)?;
        self.flush()?;
        Ok(())
    }

    /// Single-entry injection helper (no recursion).
    #[must_use = "injection result must be checked for errors"]
    fn inject_entry(&mut self, node: &mut FsNode<'_>) -> FsInjectorResult {
        self.set_root_context(node)?;
        self.inject_node(node, false)?;
        self.flush()?;
        Ok(())
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
