// SPDX-License-Identifier: MIT

use time::OffsetDateTime;

/// Standard file metadata used across filesystem abstractions.
///
/// This struct represents attributes commonly found in FAT, EXT, NTFS, and Unix filesystems,
/// abstracted into a unified interface for portable manipulation.
///
/// Fields:
/// - `read_only`: true if the file is marked as read-only.
/// - `hidden`: true if the file is hidden (e.g., `.` prefix on Unix).
/// - `system`: true if the file is used by the OS (rarely used outside FAT/NTFS).
/// - `archive`: true if marked for backup/archive.
/// - `dir`: true if this entry is a directory.
/// - `created`: creation timestamp (optional).
/// - `modified`: last modification timestamp (optional).
/// - `accessed`: last access timestamp (optional).
/// - `mode`: optional Unix-like permission bits (e.g., 0o755).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileAttributes {
    pub read_only: bool,
    pub hidden: bool,
    pub system: bool,
    pub archive: bool,
    pub dir: bool,
    pub created: Option<OffsetDateTime>,
    pub modified: Option<OffsetDateTime>,
    pub accessed: Option<OffsetDateTime>,
    pub mode: Option<u32>, // UNIX-style
}

impl FileAttributes {
    /// Creates default directory attributes (`dir = true`).
    pub fn new_dir() -> Self {
        Self {
            dir: true,
            ..Default::default()
        }
    }

    /// Creates default directory attributes (`dir = true`).
    pub fn new_file() -> Self {
        Self {
            archive: true,
            ..Default::default()
        }
    }

    /// Creates file attributes with current timestamp
    #[cfg(feature = "std")]
    pub fn new_file_now() -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
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
            dir: self.dir || other.dir,
            created: other.created.or(self.created),
            modified: other.modified.or(self.modified),
            accessed: other.accessed.or(self.accessed),
            mode: other.mode.or(self.mode),
        }
    }

    /// Sets the `read_only` flag.
    pub fn set_read_only(mut self, value: bool) -> Self {
        self.read_only = value;
        self
    }

    /// Sets the `hidden` flag.
    pub fn set_hidden(mut self, value: bool) -> Self {
        self.hidden = value;
        self
    }

    /// Sets the `system` flag.
    pub fn set_system(mut self, value: bool) -> Self {
        self.system = value;
        self
    }

    /// Compares structure only (ignores timestamps and mode).
    /// Useful for tests where timestamps are set by the filesystem.
    pub fn structural_eq(&self, other: &Self) -> bool {
        self.read_only == other.read_only
            && self.hidden == other.hidden
            && self.system == other.system
            && self.archive == other.archive
            && self.dir == other.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn test_new_dir() {
        let attr = FileAttributes::new_dir();
        assert!(attr.dir);
        assert!(!attr.read_only);
        assert!(!attr.hidden);
        assert!(!attr.system);
        assert!(!attr.archive);
        assert!(attr.created.is_none());
        assert!(attr.modified.is_none());
        assert!(attr.accessed.is_none());
        assert!(attr.mode.is_none());
    }

    #[test]
    fn test_new_file() {
        let attr = FileAttributes::new_file();
        assert!(!attr.dir);
        assert!(attr.archive);
        assert!(!attr.read_only);
        assert!(!attr.hidden);
        assert!(!attr.system);
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
            dir: false,
            created: Some(OffsetDateTime::UNIX_EPOCH),
            modified: None,
            accessed: None,
            mode: Some(0o644),
        };

        let override_attr = FileAttributes {
            read_only: true,
            hidden: true,
            system: false,
            archive: false, // base true will remain
            dir: true,
            created: None,
            modified: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1)),
            accessed: Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(2)),
            mode: None,
        };

        let merged = base.merge(&override_attr);

        assert!(merged.read_only);
        assert!(merged.hidden);
        assert!(!merged.system);
        assert!(merged.archive); // stays true
        assert!(merged.dir);
        assert_eq!(merged.created, Some(OffsetDateTime::UNIX_EPOCH));
        assert_eq!(
            merged.modified,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(1))
        );
        assert_eq!(
            merged.accessed,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::hours(2))
        );
        assert_eq!(merged.mode, Some(0o644));
    }
}
