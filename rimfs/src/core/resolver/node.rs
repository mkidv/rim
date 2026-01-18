pub use crate::core::resolver::attr::FileAttributes;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::string::ToString;
use core::fmt;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec::Vec};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FsNodeCounts {
    pub dirs: usize,
    pub files: usize,
    // optional: total bytes if you want to display it
    pub bytes: u64,
}

impl fmt::Display for FsNodeCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.dirs;
        let fi = self.files;
        let d_lbl = if d == 1 { "Dir" } else { "Dirs" };
        let f_lbl = if fi == 1 { "File" } else { "Files" };
        write!(f, "{d} {d_lbl} • {fi} {f_lbl}")
    }
}

/// Generic representation of a filesystem node (file, directory, or container).
///
/// This structure is used internally to model parsed filesystem content
/// and externally to describe tree structures for injection or comparison.
///
/// Variants:
/// - `File`  : a regular file with name, content, and attributes
/// - `Dir`   : a directory with name, children, and attributes
/// - `Container` : an anonymous container node used to group multiple nodes
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsNode {
    File {
        name: String,
        content: Vec<u8>,
        attr: FileAttributes,
    },
    Dir {
        name: String,
        children: Vec<FsNode>,
        attr: FileAttributes,
    },
    Container {
        children: Vec<FsNode>,
        attr: FileAttributes,
    },
}

impl FsNode {
    #[inline]
    pub fn name(&self) -> &str {
        match self {
            FsNode::File { name, .. } => name,
            FsNode::Dir { name, .. } => name,
            FsNode::Container { .. } => unreachable!(),
        }
    }

    #[inline]
    pub fn attr(&self) -> &FileAttributes {
        match self {
            FsNode::File { attr, .. } => attr,
            FsNode::Dir { attr, .. } => attr,
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
    pub fn is_container(&self) -> bool {
        matches!(self, FsNode::Container { .. })
    }

    pub fn sort_children_recursively(&mut self) {
        fn rank(n: &FsNode) -> u8 {
            match n {
                FsNode::Container { .. } => 0,
                FsNode::Dir { .. } => 1,
                FsNode::File { .. } => 2,
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
        fn walk(n: &FsNode, acc: &mut FsNodeCounts) {
            match n {
                FsNode::File { content, .. } => {
                    acc.files += 1;
                    acc.bytes = acc.bytes.saturating_add(content.len() as u64);
                }
                FsNode::Dir { children, .. } => {
                    acc.dirs += 1;
                    for c in children {
                        walk(c, acc);
                    }
                }
                FsNode::Container { children, .. } => {
                    // we don't count the container itself
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

    pub fn display_with<'a>(&'a self, opts: FsTreeDisplayOpts) -> FsTreeDisplay<'a> {
        FsTreeDisplay::new(self, opts)
    }

    /// Compares structure and content, ignoring timestamps and mode in attributes.
    /// Useful for tests where the filesystem sets its own timestamps.
    pub fn structural_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                FsNode::File {
                    name: n1,
                    content: c1,
                    attr: a1,
                },
                FsNode::File {
                    name: n2,
                    content: c2,
                    attr: a2,
                },
            ) => n1 == n2 && c1 == c2 && a1.structural_eq(a2),
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

    /// Creates a new file node with content.
    pub fn new_file(name: impl Into<String>, content: Vec<u8>) -> Self {
        Self::File {
            name: name.into(),
            content,
            attr: FileAttributes::default(),
        }
    }

    /// Creates a new container node (anonymous).
    pub fn new_container(children: Vec<FsNode>) -> Self {
        Self::Container {
            children,
            attr: FileAttributes::default(),
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
pub struct FsTreeDisplay<'a> {
    root: &'a FsNode,
    opts: FsTreeDisplayOpts,
}
impl<'a> FsTreeDisplay<'a> {
    pub fn new(root: &'a FsNode, opts: FsTreeDisplayOpts) -> Self {
        Self { root, opts }
    }
}
impl<'a> fmt::Display for FsTreeDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut stack: Vec<(&FsNode, String, bool, usize)> = Vec::new(); // node, prefix, last, depth
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
                FsNode::File { name, content, .. } => {
                    write!(f, "{}", truncate(name, self.opts.name_width))?;
                    if self.opts.show_attrs {
                        write!(f, " [{:?}]", node.attr())?;
                    }
                    if self.opts.show_sizes {
                        if self.opts.human_size {
                            write!(f, " ({})", pretty_bytes(content.len() as u64))?;
                        } else {
                            write!(f, " ({} bytes)", content.len())?;
                        }
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

fn pretty_bytes(n: u64) -> String {
    const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut val = n as f64;
    let mut idx = 0usize;
    while val >= 1024.0 && idx + 1 < UNITS.len() {
        val /= 1024.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{} {}", sep_u64(n), UNITS[idx])
    } else {
        format!("{:.1} {}", val, UNITS[idx])
    }
}

fn sep_u64(mut n: u64) -> String {
    // "thin" thousand separator: 12 345 678
    if n < 1_000 {
        return n.to_string();
    }
    let mut parts: Vec<String> = Vec::new();
    while n >= 1_000 {
        parts.push(format!("{:03}", (n % 1_000)));
        n /= 1_000;
    }
    parts.push(n.to_string());
    parts.reverse();
    parts.join(" ") // non-breaking thin space
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    &s[..max]
}

impl fmt::Display for FsNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        FsTreeDisplay::new(self, FsTreeDisplayOpts::default()).fmt(f)
    }
}
