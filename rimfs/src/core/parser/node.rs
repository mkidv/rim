pub use crate::core::parser::attr::FileAttributes;
use core::fmt;

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
    pub fn name(&self) -> &str {
        match self {
            FsNode::File { name, .. } => name,
            FsNode::Dir { name, .. } => name,
            FsNode::Container { .. } => unreachable!(),
        }
    }

    pub fn attr(&self) -> &FileAttributes {
        match self {
            FsNode::File { attr, .. } => attr,
            FsNode::Dir { attr, .. } => attr,
            FsNode::Container { attr, .. } => attr,
        }
    }

    pub fn is_file(&self) -> bool {
        matches!(self, FsNode::File { .. })
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, FsNode::Dir { .. })
    }

    pub fn is_container(&self) -> bool {
        matches!(self, FsNode::Container { .. })
    }

    pub fn sort_children_recursively(&mut self) {
        match self {
            FsNode::Dir { children, .. } | FsNode::Container { children, .. } => {
                children.sort_by_key(|c| c.name().to_ascii_lowercase());
                for child in children {
                    child.sort_children_recursively();
                }
            }
            _ => {}
        }
    }

    pub fn fmt_tree(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use FsNode::*;

        let mut stack = vec![(self, "".to_string(), true)];

        while let Some((node, prefix, last)) = stack.pop() {
            write!(f, "{}{}", prefix, if last { "└── " } else { "├── " })?;

            match node {
                File { name, content, .. } => {
                    writeln!(f, "{} ({} bytes)", name, content.len())?;
                }
                Dir { name, children, .. } => {
                    writeln!(f, "{name}")?;

                    let new_prefix = if last {
                        format!("{prefix}    ")
                    } else {
                        format!("{prefix}│   ")
                    };

                    // Push children in reverse order to print first in order
                    for (i, child) in children.iter().enumerate().rev() {
                        let is_last = i == children.len() - 1;
                        stack.push((child, new_prefix.clone(), is_last));
                    }
                }
                Container { children, .. } => {
                    writeln!(f, "(container)")?;

                    let new_prefix = if last {
                        format!("{prefix}    ")
                    } else {
                        format!("{prefix}│   ")
                    };

                    for (i, child) in children.iter().enumerate().rev() {
                        let is_last = i == children.len() - 1;
                        stack.push((child, new_prefix.clone(), is_last));
                    }
                }
            }
        }

        Ok(())
    }
}

impl fmt::Display for FsNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_tree(f)
    }
}
