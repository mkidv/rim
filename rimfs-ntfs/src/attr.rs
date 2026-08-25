// SPDX-License-Identifier: MIT
//! NTFS file attributes helpers (Legacy/Compatibility)

pub use crate::types::record::{AttributeType, NtfsFileNameNamespace};
pub use crate::utils::current_ntfs_time;

use crate::constant::*;
use crate::flags::*;
use crate::types::*;

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
