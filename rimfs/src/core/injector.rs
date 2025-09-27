// SPDX-License-Identifier: MIT

pub use crate::core::errors::{FsInjectorError, FsInjectorResult};
pub use crate::core::resolver::FsNode;

use crate::core::{allocator::FsHandle, resolver::attr::FileAttributes};

pub struct FsContext<Handle: FsHandle> {
    pub handle: Handle,
    pub buf: Vec<u8>,
}

impl<Handle: FsHandle> FsContext<Handle> {
    pub fn new(handle: Handle, buf: Vec<u8>) -> Self {
        Self { handle, buf }
    }
}

/*
  Injector contract (simple, no pending state):

  - write_dir(name, attr)
      * Allocate and IMMEDIATELY reserve the directory’s first cluster
        (e.g., mark EOC in FAT; for exFAT also update the bitmap).
      * Build the child directory buffer in memory (e.g., "." + ".." + EOD on FAT32;
        empty for exFAT).
      * Insert the directory entry into the CURRENT parent buffer right now
        (FAT32: size = 0; exFAT: you may insert a placeholder and remember an
        internal offset for later backpatch of DataLength).
      * Push the child context on the stack. Do NOT write to disk yet.

  - write_file(name, content, attr)
      * Allocate clusters as needed, write file content to disk,
        then append the file entry into the CURRENT directory buffer.

  - flush_current()
      * Pop the top context and WRITE ONLY that context’s buffer to disk.
        No parent entry creation or side-effects here.

  - flush()
      * Drain the stack, writing remaining directory buffers to disk.
        No additional parent entry creation here either.

  Typical recursive flow (depth-first):
      set_root_context(root)
      for each Dir:
        write_dir()      // reserves cluster + inserts entry in parent + pushes child
          [inside child]
          write_file()   // writes data + appends entry to child buffer
          write_dir()    // same as above for grandchildren, etc.
        flush_current()  // writes the child directory buffer to disk
      flush()            // final drain

  Notes:
    - Parent linkage is done immediately in write_dir(); there is no deferred
      “pending directory” step.
    - Directory buffers are written exactly once (at flush_current/flush).
    - Implementations should guarantee that allocated clusters for directories
      are not reused (reservation happens in write_dir()).
*/
pub trait FsNodeInjector<Handle: FsHandle> {
    /// Create a new directory under the current directory.
    /// Must:
    /// - allocate & reserve the child's first cluster immediately
    /// - initialize the child directory buffer in memory
    /// - append the child's directory entry to the current parent buffer now
    /// - push the child context on the internal stack
    fn write_dir(&mut self, name: &str, attr: &FileAttributes) -> FsInjectorResult;

    /// Create a file under the current directory.
    /// Should:
    /// - allocate and write the file data to disk
    /// - append the file entry to the current directory buffer
    fn write_file(
        &mut self,
        name: &str,
        content: &[u8],
        attr: &FileAttributes,
    ) -> FsInjectorResult;

    /// Initialize the root context and push it on the stack.
    /// The root context's buffer should reflect existing entries (if any).
    fn set_root_context(&mut self, node: &FsNode) -> FsInjectorResult;

    /// Recursive helper: inject a node and (optionally) its children.
    /// Uses the contract above: directories link to parent immediately,
    /// buffers are written only at flush_current/flush.
    fn inject_node(&mut self, node: &FsNode, recurse: bool) -> FsInjectorResult {
        match node {
            FsNode::File { name, content, attr } => {
                self.write_file(name, content, attr)?;
            }
            FsNode::Dir { name, children, attr } => {
                if !name.is_empty() {
                    self.write_dir(name, attr)?; // reserve + link to parent + push child
                }
                if recurse {
                    for child in children {
                        self.inject_node(child, recurse)?;
                    }
                }
                // write the child directory buffer once we are done with its contents
                self.flush_current()?;
            }
            FsNode::Container { children, .. } => {
                for child in children {
                    self.inject_node(child, recurse)?;
                }
                // container-level flush of the current context
                self.flush_current()?;
            }
        }
        Ok(())
    }

    /// Full-tree injection helper.
    fn inject_tree(&mut self, node: &FsNode) -> FsInjectorResult {
        self.set_root_context(node)?;
        self.inject_node(node, true)?;
        self.flush()?;
        Ok(())
    }

    /// Single-path injection helper (no recursion).
    fn inject_path(&mut self, node: &FsNode) -> FsInjectorResult {
        self.set_root_context(node)?;
        self.inject_node(node, false)?;
        self.flush()?;
        Ok(())
    }

    /// Write the current (top-of-stack) directory buffer to disk and pop it.
    /// No parent entry creation here.
    fn flush_current(&mut self) -> FsInjectorResult {
        Ok(())
    }

    /// Drain and write all remaining directory buffers to disk.
    /// No parent entry creation here either.
    fn flush(&mut self) -> FsInjectorResult {
        Ok(())
    }
}
