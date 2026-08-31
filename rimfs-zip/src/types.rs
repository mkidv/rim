// SPDX-License-Identifier: MIT
#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
use alloc::string::String;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use rimfs_core::allocator::FsHandle;
use time::OffsetDateTime;

/// Magic signatures for ZIP file structures.
pub const LOCAL_FILE_HEADER_SIG: u32 = 0x0403_4b50;
pub const CENTRAL_DIR_HEADER_SIG: u32 = 0x0201_4b50;
pub const END_OF_CENTRAL_DIR_SIG: u32 = 0x0605_4b50;
pub const ZIP64_END_OF_CENTRAL_DIR_SIG: u32 = 0x0606_4b50;
pub const ZIP64_END_OF_CENTRAL_DIR_LOCATOR_SIG: u32 = 0x0706_4b50;

/// Standard ZIP compression methods.
pub const METHOD_STORE: u16 = 0;
pub const METHOD_DEFLATE: u16 = 8;

/// Version specifications.
pub const VERSION_MADE_BY_UNIX: u16 = 0x031e; // Unix, version 3.0
pub const VERSION_NEEDED_DEFAULT: u16 = 20; // 2.0 (file system folders, deflate)
pub const VERSION_NEEDED_ZIP64: u16 = 45; // 4.5 (ZIP64 format extensions)

/// General purpose bit flags.
pub const FLAG_UTF8_FILENAME: u16 = 0x0800; // Bit 11: Language encoding flag (UTF-8)

/// Extra field header IDs.
pub const EXTRA_ZIP64_ID: u16 = 0x0001;
pub const EXTRA_EXTENDED_TIMESTAMP_ID: u16 = 0x5455; // "UT"
pub const EXTRA_UNIX_UID_GID_ID: u16 = 0x7875; // "ux" (Info-ZIP Unix New)

/// Fixed byte sizes of ZIP structures.
pub const LOCAL_FILE_HEADER_FIXED_SIZE: usize = 30;
pub const CENTRAL_DIR_HEADER_FIXED_SIZE: usize = 46;
pub const END_OF_CENTRAL_DIR_FIXED_SIZE: usize = 22;
pub const ZIP64_EOCD_FIXED_SIZE: usize = 56;
pub const ZIP64_LOCATOR_FIXED_SIZE: usize = 20;

/// A handle representing an offset in a ZIP archive.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ZipHandle(pub u64);
impl FsHandle for ZipHandle {}

/// Decoded in-memory representation of a Central Directory entry.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    pub name: String,
    pub compression_method: u16,
    pub mtime_dos: u16,
    pub mdate_dos: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub local_header_offset: u64,
    pub external_attributes: u32,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub unix_mode: Option<u32>,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub timestamp: Option<OffsetDateTime>,
}

impl ZipEntry {
    #[inline]
    pub fn is_directory(&self) -> bool {
        self.is_dir || self.name.ends_with('/')
    }

    #[inline]
    pub fn is_symbolic_link(&self) -> bool {
        self.is_symlink || (self.external_attributes >> 16) & 0o170000 == 0o120000
    }
}

/// Converts an [`OffsetDateTime`] into MS-DOS (time, date) format.
pub fn datetime_to_dos(dt: OffsetDateTime) -> (u16, u16) {
    let year = dt.year();
    let dos_year = if year < 1980 {
        0
    } else {
        ((year - 1980) as u16).min(127)
    };
    let dos_month = dt.month() as u16;
    let dos_day = dt.day() as u16;
    let date = (dos_year << 9) | (dos_month << 5) | dos_day;

    let hour = dt.hour() as u16;
    let min = dt.minute() as u16;
    let sec = (dt.second() / 2) as u16;
    let time = (hour << 11) | (min << 5) | sec;

    (time, date)
}

/// Converts MS-DOS (time, date) format into an [`OffsetDateTime`].
pub fn dos_to_datetime(time: u16, date: u16) -> Option<OffsetDateTime> {
    let year = 1980 + ((date >> 9) & 0x7F) as i32;
    let month_val = ((date >> 5) & 0x0F) as u8;
    let day = (date & 0x1F) as u8;

    let month = time::Month::try_from(month_val).ok()?;
    let hour = ((time >> 11) & 0x1F) as u8;
    let min = ((time >> 5) & 0x3F) as u8;
    let sec = ((time & 0x1F) * 2).min(59) as u8;

    let date_obj = time::Date::from_calendar_date(year, month, day).ok()?;
    let time_obj = time::Time::from_hms(hour, min, sec).ok()?;
    Some(OffsetDateTime::new_utc(date_obj, time_obj))
}

/// Encodes an Extended Timestamp (`0x5455`) extra field.
#[cfg(feature = "alloc")]
pub fn encode_extended_timestamp_extra(dt: OffsetDateTime, _is_local_header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&EXTRA_EXTENDED_TIMESTAMP_ID.to_le_bytes());
    let flag = 0x01u8; // bit 0: modtime is present
    let len: u16 = 5;
    out.extend_from_slice(&len.to_le_bytes());
    out.push(flag);
    let mtime = dt.unix_timestamp() as i32;
    out.extend_from_slice(&mtime.to_le_bytes());
    out
}

/// Encodes an Info-ZIP Unix New (`0x7875`) extra field containing UID and GID.
#[cfg(feature = "alloc")]
pub fn encode_unix_uid_gid_extra(uid: u32, gid: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(16);
    out.extend_from_slice(&EXTRA_UNIX_UID_GID_ID.to_le_bytes());
    let len: u16 = 11; // version (1) + uid_size (1) + uid (4) + gid_size (1) + gid (4)
    out.extend_from_slice(&len.to_le_bytes());
    out.push(1); // version 1
    out.push(4); // UID size 4 bytes
    out.extend_from_slice(&uid.to_le_bytes());
    out.push(4); // GID size 4 bytes
    out.extend_from_slice(&gid.to_le_bytes());
    out
}

/// Decodes Extra Fields to extract Extended Timestamp, Unix UID/GID, and ZIP64 offsets.
pub fn parse_extra_fields(extra: &[u8], zip_entry: &mut ZipEntry, is_local: bool) {
    let mut offset = 0;
    while offset + 4 <= extra.len() {
        let header_id = u16::from_le_bytes([extra[offset], extra[offset + 1]]);
        let data_size = u16::from_le_bytes([extra[offset + 2], extra[offset + 3]]) as usize;
        offset += 4;

        if offset + data_size > extra.len() {
            break;
        }
        let data = &extra[offset..offset + data_size];
        offset += data_size;

        match header_id {
            EXTRA_EXTENDED_TIMESTAMP_ID => {
                if !data.is_empty() {
                    let flags = data[0];
                    if (flags & 0x01) != 0 && data.len() >= 5 {
                        let mtime = i32::from_le_bytes([data[1], data[2], data[3], data[4]]);
                        if let Ok(dt) = OffsetDateTime::from_unix_timestamp(mtime as i64) {
                            zip_entry.timestamp = Some(dt);
                        }
                    }
                }
            }
            EXTRA_UNIX_UID_GID_ID => {
                // Info-ZIP Unix New (0x7875)
                if data.len() >= 3 && data[0] == 1 {
                    let uid_size = data[1] as usize;
                    let mut cursor = 2;
                    if cursor + uid_size <= data.len() {
                        let uid = match uid_size {
                            2 => u16::from_le_bytes([data[cursor], data[cursor + 1]]) as u32,
                            4 => u32::from_le_bytes([
                                data[cursor],
                                data[cursor + 1],
                                data[cursor + 2],
                                data[cursor + 3],
                            ]),
                            _ => 0,
                        };
                        zip_entry.uid = Some(uid);
                        cursor += uid_size;

                        if cursor < data.len() {
                            let gid_size = data[cursor] as usize;
                            cursor += 1;
                            if cursor + gid_size <= data.len() {
                                let gid = match gid_size {
                                    2 => {
                                        u16::from_le_bytes([data[cursor], data[cursor + 1]]) as u32
                                    }
                                    4 => u32::from_le_bytes([
                                        data[cursor],
                                        data[cursor + 1],
                                        data[cursor + 2],
                                        data[cursor + 3],
                                    ]),
                                    _ => 0,
                                };
                                zip_entry.gid = Some(gid);
                            }
                        }
                    }
                }
            }
            EXTRA_ZIP64_ID => {
                let mut cursor = 0;
                if zip_entry.uncompressed_size == 0xFFFF_FFFF && cursor + 8 <= data.len() {
                    zip_entry.uncompressed_size = u64::from_le_bytes([
                        data[cursor],
                        data[cursor + 1],
                        data[cursor + 2],
                        data[cursor + 3],
                        data[cursor + 4],
                        data[cursor + 5],
                        data[cursor + 6],
                        data[cursor + 7],
                    ]);
                    cursor += 8;
                }
                if zip_entry.compressed_size == 0xFFFF_FFFF && cursor + 8 <= data.len() {
                    zip_entry.compressed_size = u64::from_le_bytes([
                        data[cursor],
                        data[cursor + 1],
                        data[cursor + 2],
                        data[cursor + 3],
                        data[cursor + 4],
                        data[cursor + 5],
                        data[cursor + 6],
                        data[cursor + 7],
                    ]);
                    cursor += 8;
                }
                if !is_local
                    && zip_entry.local_header_offset == 0xFFFF_FFFF
                    && cursor + 8 <= data.len()
                {
                    zip_entry.local_header_offset = u64::from_le_bytes([
                        data[cursor],
                        data[cursor + 1],
                        data[cursor + 2],
                        data[cursor + 3],
                        data[cursor + 4],
                        data[cursor + 5],
                        data[cursor + 6],
                        data[cursor + 7],
                    ]);
                }
            }
            _ => {}
        }
    }
}
