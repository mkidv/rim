// SPDX-License-Identifier: MIT
//! NTFS file attributes helpers (Legacy/Compatibility)

pub use crate::types::record::{AttributeType, NtfsFileNameNamespace};
pub use crate::utils::current_ntfs_time;

use crate::constant::*;
use crate::core::resolver::attr::FileAttributes;
use crate::flags::*;
use crate::types::*;

pub trait NtfsFileAttributesExt {
    fn as_ntfs_attr(&self) -> NtfsFileAttributes;
    fn from_ntfs_attr(attr: NtfsFileAttributes) -> Self;
}

impl NtfsFileAttributesExt for FileAttributes {
    fn as_ntfs_attr(&self) -> NtfsFileAttributes {
        let mut flags = NtfsFileAttributes::empty();
        if self.read_only {
            flags |= NtfsFileAttributes::READ_ONLY;
        }
        if self.hidden {
            flags |= NtfsFileAttributes::HIDDEN;
        }
        if self.system {
            flags |= NtfsFileAttributes::SYSTEM;
        }
        if self.archive {
            flags |= NtfsFileAttributes::ARCHIVE;
        }
        if self.is_dir() {
            flags |= NtfsFileAttributes::DIRECTORY;
        }
        flags
    }

    fn from_ntfs_attr(attr: NtfsFileAttributes) -> Self {
        let is_dir = attr.contains(NtfsFileAttributes::DIRECTORY);
        let mut fa = if is_dir {
            FileAttributes::new_dir()
        } else {
            FileAttributes::new_file()
        };
        fa.read_only = attr.contains(NtfsFileAttributes::READ_ONLY);
        fa.hidden = attr.contains(NtfsFileAttributes::HIDDEN);
        fa.system = attr.contains(NtfsFileAttributes::SYSTEM);
        fa.archive = attr.contains(NtfsFileAttributes::ARCHIVE);
        fa
    }
}

/// Build $VOLUME_INFORMATION attribute
pub fn build_volume_information() -> VolumeInformation {
    VolumeInformation {
        reserved: 0,
        major_version: 3, // NTFS 3.1
        minor_version: 1,
        flags: NtfsVolumeFlags::empty().bits(),
    }
}

/// Build an MFT record header (Legacy)
pub fn build_mft_record_header(
    record_number: u64,
    flags: MftRecordFlags,
    attrs_offset: u16,
    bytes_used: u32,
    bytes_allocated: u32,
) -> MftRecordHeader {
    MftRecordHeader {
        signature: NTFS_FILE_SIGNATURE,
        usa_offset: 48,
        usa_count: 3,
        lsn: 0,
        sequence_number: (record_number as u16).max(1),
        link_count: 1,
        attrs_offset,
        flags: flags.bits(),
        bytes_used,
        bytes_allocated,
        base_file_record: 0,
        next_attr_id: 1,
        reserved: 0,
        mft_record_number: record_number as u32,
    }
}
