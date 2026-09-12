// SPDX-License-Identifier: MIT

//! Common filesystem entry attributes and timestamps.

use time::OffsetDateTime;

/// Represents the kind of a filesystem node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeKind {
    #[default]
    Regular,
    Directory,
    Symlink,
    Fifo,
    Socket,
    CharDevice,
    BlockDevice,
}

impl NodeKind {
    #[inline]
    pub fn is_dir(&self) -> bool {
        matches!(self, NodeKind::Directory)
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        matches!(self, NodeKind::Regular)
    }

    #[inline]
    pub fn is_symlink(&self) -> bool {
        matches!(self, NodeKind::Symlink)
    }
}

/// Standard file metadata used across filesystem abstractions.
///
/// This struct represents attributes commonly found in FAT, EXT, NTFS, and Unix filesystems,
/// abstracted into a unified interface for portable manipulation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileAttributes {
    pub read_only: bool,
    pub hidden: bool,
    pub system: bool,
    pub archive: bool,
    pub kind: NodeKind,
    pub contiguous: bool, // RimFAT: File is contiguous
    pub created: Option<OffsetDateTime>,
    pub modified: Option<OffsetDateTime>,
    pub accessed: Option<OffsetDateTime>,
    pub mode: Option<u32>, // UNIX-style (perms + special bits)
    pub uid: Option<u32>,
    pub gid: Option<u32>,
}

impl FileAttributes {
    /// Creates default directory attributes.
    pub fn new_dir() -> Self {
        Self {
            kind: NodeKind::Directory,
            ..Default::default()
        }
    }

    /// Creates default regular file attributes.
    pub fn new_file() -> Self {
        Self {
            kind: NodeKind::Regular,
            archive: true,
            ..Default::default()
        }
    }

    /// Creates default symlink attributes.
    pub fn new_symlink() -> Self {
        Self {
            kind: NodeKind::Symlink,
            ..Default::default()
        }
    }

    /// Creates file attributes with current timestamp
    #[cfg(feature = "std")]
    pub fn new_file_now() -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            kind: NodeKind::Regular,
            archive: true,
            created: Some(now),
            modified: Some(now),
            accessed: Some(now),
            ..Default::default()
        }
    }

    /// Merges another [`FileAttributes`] into `self`.
    ///
    /// For boolean fields, `other`'s `true` values override `self`.
    /// For optional fields, `Some` values in `other` override `self`.
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            read_only: self.read_only || other.read_only,
            hidden: self.hidden || other.hidden,
            system: self.system || other.system,
            archive: self.archive || other.archive,
            kind: if other.kind != NodeKind::Regular {
                other.kind
            } else {
                self.kind
            },
            created: other.created.or(self.created),
            modified: other.modified.or(self.modified),
            accessed: other.accessed.or(self.accessed),
            contiguous: self.contiguous || other.contiguous,
            mode: other.mode.or(self.mode),
            uid: other.uid.or(self.uid),
            gid: other.gid.or(self.gid),
        }
    }

    pub fn set_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    pub fn set_hidden(mut self, value: bool) -> Self {
        self.hidden = value;
        self
    }

    pub fn set_system(mut self, value: bool) -> Self {
        self.system = value;
        self
    }

    #[inline]
    pub fn is_dir(&self) -> bool {
        self.kind.is_dir()
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        self.kind.is_file()
    }

    #[inline]
    pub fn is_symlink(&self) -> bool {
        self.kind.is_symlink()
    }

    #[inline]
    pub fn is_readonly(&self) -> bool {
        self.read_only
    }

    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    #[inline]
    pub fn is_system(&self) -> bool {
        self.system
    }

    #[inline]
    pub fn is_archive(&self) -> bool {
        self.archive
    }

    #[inline]
    pub fn is_contiguous(&self) -> bool {
        self.contiguous
    }

    /// Compares structure only (ignores timestamps, mode, uid, gid).
    /// Useful for tests where timestamps are set by the filesystem.
    pub fn structural_eq(&self, other: &Self) -> bool {
        self.read_only == other.read_only
            && self.hidden == other.hidden
            && self.system == other.system
            && self.archive == other.archive
            && self.kind == other.kind
            && self.contiguous == other.contiguous
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn test_new_dir() {
        let attr = FileAttributes::new_dir();
        assert!(attr.is_dir());
        assert!(!attr.is_file());
        assert!(!attr.is_symlink());
        assert_eq!(attr.kind, NodeKind::Directory);
        assert!(!attr.read_only);
        assert!(!attr.hidden);
        assert!(!attr.system);
        assert!(!attr.archive);
        assert!(attr.created.is_none());
        assert!(attr.modified.is_none());
        assert!(attr.accessed.is_none());
        assert!(attr.mode.is_none());
        assert!(attr.uid.is_none());
        assert!(attr.gid.is_none());
    }

    #[test]
    fn test_new_file() {
        let attr = FileAttributes::new_file();
        assert!(!attr.is_dir());
        assert!(attr.is_file());
        assert!(!attr.is_symlink());
        assert_eq!(attr.kind, NodeKind::Regular);
        assert!(attr.archive);
        assert!(!attr.read_only);
        assert!(!attr.hidden);
        assert!(!attr.system);
    }

    #[test]
    fn test_new_symlink() {
        let attr = FileAttributes::new_symlink();
        assert!(!attr.is_dir());
        assert!(!attr.is_file());
        assert!(attr.is_symlink());
        assert_eq!(attr.kind, NodeKind::Symlink);
    }

    #[test]
    fn test_set_read_only_hidden_system() {
        let attr = FileAttributes::new_file()
            .set_read_only(true)
            .set_hidden(true)
            .set_system(true);

        assert!(attr.read_only);
        assert!(attr.hidden);
        assert!(attr.system);
        assert!(attr.archive);
    }

    #[test]
    fn test_merge_attributes() {
        let base = FileAttributes {
            read_only: false,
            hidden: false,
            system: false,
            archive: true,
            kind: NodeKind::Regular,
            created: Some(OffsetDateTime::UNIX_EPOCH),
            modified: None,
            accessed: None,
            contiguous: false,
            mode: Some(0o644),
            uid: Some(1000),
            gid: Some(1000),
        };

        let override_attr = FileAttributes {
            read_only: true,
            hidden: true,
            system: false,
            archive: false, // base true will remain
            kind: NodeKind::Directory,
            created: None,
            modified: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1)),
            accessed: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(2)),
            contiguous: true,
            mode: Some(0), // explicit mode 0 must override
            uid: Some(0),  // explicit uid 0 must override
            gid: None,
        };

        let merged = base.merge(&override_attr);

        assert!(merged.read_only);
        assert!(merged.hidden);
        assert!(!merged.system);
        assert!(merged.archive); // stays true
        assert!(merged.is_dir());
        assert_eq!(merged.created, Some(OffsetDateTime::UNIX_EPOCH));
        assert_eq!(
            merged.modified,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1))
        );
        assert_eq!(
            merged.accessed,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(2))
        );
        assert_eq!(merged.mode, Some(0));
        assert_eq!(merged.uid, Some(0));
        assert_eq!(merged.gid, Some(1000));
    }
}
