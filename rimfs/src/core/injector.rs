// SPDX-License-Identifier: MIT

pub use crate::core::error::{FsInjectorError, FsInjectorResult};
pub use crate::core::parser::FsNode;

use crate::core::{allocator::FsHandle, parser::attr::FileAttributes};

pub struct FsContext<Handle: FsHandle> {
    pub handle: Handle,
    pub buf: Vec<u8>,
}

impl<Handle: FsHandle> FsContext<Handle> {
    pub fn new(handle: Handle, buf: Vec<u8>) -> Self {
        Self { handle, buf }
    }
}

/// Trait for injecting nodes (files and directories) into a filesystem image.
///
/// This trait allows recursive, structured insertion of nodes using a stateful context stack.
/// Implementations must manage a stack of `FsNodeContext<H>` during injection.
///
/// Typical flow:
/// - For each dir/file:
///     - `dir()` → `flush_current()`
///     - `file()`
/// - `flush()`
pub trait FsNodeInjector<Handle: FsHandle> {
    /// Start a new directory with custom attributes (e.g., timestamps, permissions).
    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult;

    /// Inject a file and write its content along with its attributes.
    fn write_file(&mut self, name: &str, content: &[u8], attr: &FileAttributes)
    -> FsInjectorResult;

    fn set_root_context(&mut self, node: &FsNode) -> FsInjectorResult;

    fn inject_node(&mut self, node: &FsNode, recurse: bool) -> FsInjectorResult {
        match node {
            FsNode::File {
                name,
                content,
                attr,
            } => {
                self.write_file(name, content, attr)?;
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
            FsNode::Container { children, .. } => {
                for child in children {
                    self.inject_node(child, recurse)?;
                }
                self.flush_current()?;
            }
        }
        Ok(())
    }

    fn inject_tree(&mut self, node: &FsNode) -> FsInjectorResult {
        self.set_root_context(node)?;
        self.inject_node(node, true)?;
        self.flush()?;
        Ok(())
    }

    fn inject_path(&mut self, node: &FsNode) -> FsInjectorResult {
        self.set_root_context(node)?;
        self.inject_node(node, false)?;
        self.flush()?;
        Ok(())
    }
    /// Finalize the current operation.
    /// Default impl is a no-op.
    fn flush_current(&mut self) -> FsInjectorResult {
        Ok(())
    }
    /// Finalize the injection session.
    /// Default impl is a no-op.
    fn flush(&mut self) -> FsInjectorResult {
        Ok(())
    }
}
