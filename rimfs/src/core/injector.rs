// SPDX-License-Identifier: MIT

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use ::alloc::vec::Vec;

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

use rimio::prelude::RimIO;

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

  - write_file(name, source, size, attr)
      * Allocate clusters as needed for `size`.
      * Stream `source` content to disk using `copy_range_smart`.
      * Append the file entry into the CURRENT directory buffer.

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
*/
pub trait FsNodeInjector<Handle: FsHandle> {
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
        source: &mut dyn RimIO,
        size: u64,
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
            FsNode::File {
                name,
                content,
                attr,
            } => {
                // Wrapper for compatibility with in-memory trees (tests, small injections)
                #[cfg(all(feature = "alloc", feature = "mem"))]
                {
                    let mut data = content.clone();
                    let mut io = rimio::prelude::MemRimIO::new(&mut data);
                    self.write_file(name, &mut io, content.len() as u64, attr)?;
                }
                #[cfg(all(feature = "alloc", not(feature = "mem")))]
                {
                    let _ = (name, content, attr);
                    return Err(FsInjectorError::Other(
                        "In-memory injection requires mem feature",
                    ));
                }
                #[cfg(not(feature = "alloc"))]
                {
                    let _ = (name, content, attr);
                    return Err(FsInjectorError::Other("In-memory injection requires alloc"));
                }
            }
            FsNode::Dir {
                name,
                children,
                attr,
            } => {
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
    #[must_use = "injection result must be checked for errors"]
    fn inject_tree(&mut self, node: &FsNode) -> FsInjectorResult {
        self.set_root_context(node)?;
        self.inject_node(node, true)?;
        self.flush()?;
        Ok(())
    }

    /// Single-path injection helper (no recursion).
    #[must_use = "injection result must be checked for errors"]
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
