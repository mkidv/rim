// SPDX-License-Identifier: MIT

//! Logical filesystem node representation (files, directories, symlinks).

pub use crate::resolver::attr::FileAttributes;
use core::fmt;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::{boxed::Box, string::String, vec::Vec};

use rimio::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FsNodeCounts {
    pub dirs: usize,
    pub files: usize,
    pub symlinks: usize,
    pub bytes: u64,
}

impl fmt::Display for FsNodeCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.dirs;
        let fi = self.files;
        let d_lbl = if d == 1 { "Dir" } else { "Dirs" };
        let f_lbl = if fi == 1 { "File" } else { "Files" };
        if self.symlinks > 0 {
            let s = self.symlinks;
            let s_lbl = if s == 1 { "Symlink" } else { "Symlinks" };
            write!(f, "{d} {d_lbl} • {fi} {f_lbl} • {s} {s_lbl}")
        } else {
            write!(f, "{d} {d_lbl} • {fi} {f_lbl}")
        }
    }
}

/// Generic representation of a filesystem node (file, directory, symlink, or container).
///
/// Variants:
/// - `File`  : a regular file with name, streaming readable source, and attributes
/// - `Dir`   : a directory with name, children, and attributes
/// - `Symlink` : a symbolic link with name, target path, and attributes
/// - `Container` : an anonymous container node used to group multiple nodes
pub enum FsNode<'a> {
    File {
        name: String,
        source: Box<dyn RimRead + 'a>,
        attr: FileAttributes,
    },
    Dir {
        name: String,
        children: Vec<FsNode<'a>>,
        attr: FileAttributes,
    },
    Symlink {
        name: String,
        target: String,
        attr: FileAttributes,
    },
    Container {
        children: Vec<FsNode<'a>>,
        attr: FileAttributes,
    },
}

impl<'a> core::fmt::Debug for FsNode<'a> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FsNode::File { name, attr, .. } => f
                .debug_struct("File")
                .field("name", name)
                .field("attr", attr)
                .finish_non_exhaustive(),
            FsNode::Dir {
                name,
                children,
                attr,
            } => f
                .debug_struct("Dir")
                .field("name", name)
                .field("children", children)
                .field("attr", attr)
                .finish(),
            FsNode::Symlink { name, target, attr } => f
                .debug_struct("Symlink")
                .field("name", name)
                .field("target", target)
                .field("attr", attr)
                .finish(),
            FsNode::Container { children, attr } => f
                .debug_struct("Container")
                .field("children", children)
                .field("attr", attr)
                .finish(),
        }
    }
}

impl<'a> FsNode<'a> {
    #[inline]
    pub fn name(&self) -> &str {
        match self {
            FsNode::File { name, .. } => name,
            FsNode::Dir { name, .. } => name,
            FsNode::Symlink { name, .. } => name,
            FsNode::Container { .. } => unreachable!(),
        }
    }

    #[inline]
    pub fn attr(&self) -> &FileAttributes {
        match self {
            FsNode::File { attr, .. } => attr,
            FsNode::Dir { attr, .. } => attr,
            FsNode::Symlink { attr, .. } => attr,
            FsNode::Container { attr, .. } => attr,
        }
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        matches!(self, FsNode::File { .. })
    }
    #[inline]
    pub fn is_dir(&self) -> bool {
        matches!(self, FsNode::Dir { .. })
    }
    #[inline]
    pub fn is_symlink(&self) -> bool {
        matches!(self, FsNode::Symlink { .. })
    }
    #[inline]
    pub fn is_container(&self) -> bool {
        matches!(self, FsNode::Container { .. })
    }

    pub fn sort_children_recursively(&mut self) {
        fn rank(n: &FsNode<'_>) -> u8 {
            match n {
                FsNode::Container { .. } => 0,
                FsNode::Dir { .. } => 1,
                FsNode::Symlink { .. } => 2,
                FsNode::File { .. } => 3,
            }
        }
        match self {
            FsNode::Dir { children, .. } | FsNode::Container { children, .. } => {
                children.sort_by(|a, b| {
                    rank(a).cmp(&rank(b)).then_with(|| {
                        a.name()
                            .to_ascii_lowercase()
                            .cmp(&b.name().to_ascii_lowercase())
                    })
                });
                for c in children {
                    c.sort_children_recursively();
                }
            }
            _ => {}
        }
    }

    pub fn counts(&self) -> FsNodeCounts {
        fn walk(n: &FsNode<'_>, acc: &mut FsNodeCounts) {
            match n {
                FsNode::File { .. } => {
                    acc.files += 1;
                }
                FsNode::Dir { children, .. } => {
                    acc.dirs += 1;
                    for c in children {
                        walk(c, acc);
                    }
                }
                FsNode::Symlink { .. } => {
                    acc.symlinks += 1;
                }
                FsNode::Container { children, .. } => {
                    for c in children {
                        walk(c, acc);
                    }
                }
            }
        }
        let mut out = FsNodeCounts::default();
        walk(self, &mut out);
        out
    }

    pub fn display_with<'b>(&'b self, opts: FsTreeDisplayOpts) -> FsTreeDisplay<'b, 'a> {
        FsTreeDisplay::new(self, opts)
    }

    /// Compares structure and attributes, ignoring volatile timestamps.
    pub fn structural_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                FsNode::File {
                    name: n1, attr: a1, ..
                },
                FsNode::File {
                    name: n2, attr: a2, ..
                },
            ) => n1 == n2 && a1.structural_eq(a2),
            (
                FsNode::Dir {
                    name: n1,
                    children: ch1,
                    attr: a1,
                },
                FsNode::Dir {
                    name: n2,
                    children: ch2,
                    attr: a2,
                },
            ) => {
                n1 == n2
                    && a1.structural_eq(a2)
                    && ch1.len() == ch2.len()
                    && ch1.iter().zip(ch2).all(|(c1, c2)| c1.structural_eq(c2))
            }
            (
                FsNode::Symlink {
                    name: n1,
                    target: t1,
                    attr: a1,
                },
                FsNode::Symlink {
                    name: n2,
                    target: t2,
                    attr: a2,
                },
            ) => n1 == n2 && t1 == t2 && a1.structural_eq(a2),
            (
                FsNode::Container {
                    children: ch1,
                    attr: a1,
                },
                FsNode::Container {
                    children: ch2,
                    attr: a2,
                },
            ) => {
                a1.structural_eq(a2)
                    && ch1.len() == ch2.len()
                    && ch1.iter().zip(ch2).all(|(c1, c2)| c1.structural_eq(c2))
            }
            _ => false,
        }
    }

    /// Creates a new directory node.
    pub fn new_dir(name: impl Into<String>) -> Self {
        Self::Dir {
            name: name.into(),
            children: Vec::new(),
            attr: FileAttributes::new_dir(),
        }
    }

    /// Creates a new file node from an in-memory byte buffer.
    pub fn new_file(name: impl Into<String>, content: Vec<u8>) -> Self {
        Self::File {
            name: name.into(),
            source: Box::new(VecRimIO::new(content)),
            attr: FileAttributes::default(),
        }
    }

    /// Creates a new file node from an in-memory borrowed slice.
    pub fn new_file_from_slice(name: impl Into<String>, slice: &'a [u8]) -> Self {
        Self::File {
            name: name.into(),
            source: Box::new(SliceRimIO::new(slice)),
            attr: FileAttributes::default(),
        }
    }

    /// Creates a new file node from any `RimRead` source.
    pub fn new_file_from_source(
        name: impl Into<String>,
        source: Box<dyn RimRead + 'a>,
        attr: FileAttributes,
    ) -> Self {
        Self::File {
            name: name.into(),
            source,
            attr,
        }
    }

    /// Creates a new symlink node.
    pub fn new_symlink(name: impl Into<String>, target: impl Into<String>) -> Self {
        Self::Symlink {
            name: name.into(),
            target: target.into(),
            attr: FileAttributes::new_symlink(),
        }
    }

    /// Creates an anonymous root-like container node.
    pub fn new_container(children: Vec<FsNode<'a>>) -> Self {
        Self::Container {
            children,
            attr: FileAttributes::new_dir(),
        }
    }
}

/// Display Options
#[derive(Clone, Copy)]
pub struct FsTreeDisplayOpts {
    pub max_depth: usize,  // 0 = unlimited
    pub max_lines: usize,  // 0 = unlimited
    pub name_width: usize, // name truncation
    pub show_sizes: bool,
    pub human_size: bool,
    pub show_attrs: bool,
}

impl FsTreeDisplayOpts {
    pub fn new(
        max_depth: usize,
        max_lines: usize,
        name_width: usize,
        show_sizes: bool,
        human_size: bool,
        show_attrs: bool,
    ) -> Self {
        Self {
            max_depth,
            max_lines,
            name_width,
            show_sizes,
            human_size,
            show_attrs,
        }
    }
}
impl Default for FsTreeDisplayOpts {
    fn default() -> Self {
        Self::new(0, 0, 40, true, true, false)
    }
}

/// Formatter / Display
pub struct FsTreeDisplay<'a, 'b> {
    root: &'a FsNode<'b>,
    opts: FsTreeDisplayOpts,
}
impl<'a, 'b> FsTreeDisplay<'a, 'b> {
    pub fn new(root: &'a FsNode<'b>, opts: FsTreeDisplayOpts) -> Self {
        Self { root, opts }
    }
}
impl<'a, 'b> fmt::Display for FsTreeDisplay<'a, 'b> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut stack: Vec<(&FsNode<'b>, String, bool, usize)> = Vec::new(); // node, prefix, last, depth
        stack.push((self.root, String::new(), true, 0));

        let mut printed = 0usize;

        while let Some((node, prefix, last, depth)) = stack.pop() {
            if self.opts.max_lines != 0 && printed >= self.opts.max_lines {
                writeln!(f, "{prefix}    … (+more)")?;
                break;
            }
            if self.opts.max_depth != 0 && depth > self.opts.max_depth {
                continue;
            }

            write!(f, "{}{}", prefix, if last { "└── " } else { "├── " })?;

            match node {
                FsNode::File { name, .. } => {
                    write!(f, "{}", truncate(name, self.opts.name_width))?;
                    if self.opts.show_attrs {
                        write!(f, " [{:?}]", node.attr())?;
                    }
                    writeln!(f)?;
                    printed += 1;
                }
                FsNode::Symlink { name, target, .. } => {
                    write!(f, "{} -> {}", truncate(name, self.opts.name_width), target)?;
                    if self.opts.show_attrs {
                        write!(f, " [{:?}]", node.attr())?;
                    }
                    writeln!(f)?;
                    printed += 1;
                }
                FsNode::Dir { name, children, .. } => {
                    writeln!(f, "{}", truncate(name, self.opts.name_width))?;

                    let mut new_prefix = String::with_capacity(prefix.len() + 4);
                    new_prefix.push_str(&prefix);
                    new_prefix.push_str(if last { "    " } else { "│   " });

                    for (i, child) in children.iter().enumerate().rev() {
                        let is_last = i == children.len() - 1;
                        stack.push((child, new_prefix.clone(), is_last, depth + 1));
                    }
                    printed += 1;
                }
                FsNode::Container { children, .. } => {
                    writeln!(f, "(container)")?;

                    let mut new_prefix = String::with_capacity(prefix.len() + 4);
                    new_prefix.push_str(&prefix);
                    new_prefix.push_str(if last { "    " } else { "│   " });

                    for (i, child) in children.iter().enumerate().rev() {
                        let is_last = i == children.len() - 1;
                        stack.push((child, new_prefix.clone(), is_last, depth + 1));
                    }
                    printed += 1;
                }
            }
        }
        Ok(())
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    &s[..max]
}

impl<'a> fmt::Display for FsNode<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        FsTreeDisplay::new(self, FsTreeDisplayOpts::default()).fmt(f)
    }
}
