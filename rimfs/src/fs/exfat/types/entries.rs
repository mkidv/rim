// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    core::{errors::*, resolver::*, utils::time_utils},
    fs::exfat::{constant::*, upcase::UpcaseHandle, utils},
};

#[derive(Debug, Clone)]
pub struct ExFatEntries {
    pub primary: ExFatPrimaryEntry,
    pub stream: ExFatStreamEntry,
    pub names: Vec<ExFatNameEntry>,
}

impl ExFatEntries {
    pub fn name(&self) -> FsParsingResult<String> {
        decode_name(&self.names)
    }

    pub fn name_bytes_eq(&self, target: &str) -> bool {
        if let Ok(name) = self.name() {
            name.eq_ignore_ascii_case(target)
        } else {
            false
        }
    }

    pub fn size(&self) -> usize {
        self.stream.data_length as usize
    }

    pub fn attr(&self) -> FileAttributes {
        FileAttributes::from_exfat_attr(self.primary.file_attributes)
    }

    pub fn is_dir(&self) -> bool {
        self.attr().dir
    }

    pub fn first_cluster(&self) -> u32 {
        self.stream.first_cluster
    }

    pub fn dir(name: &str, cluster: u32, attr: &FileAttributes, upcase: &UpcaseHandle) -> Self {
        let names = name_entries(name);

        let name_length = name_length_utf16(name);

        let secondary_count = 1 + names.len() as u8;

        let mut primary = ExFatPrimaryEntry::new(attr, secondary_count);

        let name_hash = compute_name_hash(name, upcase);

        let stream = ExFatStreamEntry::new(cluster, 0, name_length, name_hash);

        primary.compute_set_checksum(&stream, &names);

        Self {
            primary,
            stream,
            names,
        }
    }

    pub fn dir_with_len(
        name: &str,
        first_cluster: u32,
        attr: &FileAttributes,
        data_len: u64,
        upcase: &UpcaseHandle,
    ) -> Self {
        let names = name_entries(name);
        let name_length = name_length_utf16(name);

        let secondary_count = 1 + names.len() as u8;

        let mut primary = ExFatPrimaryEntry::new(attr, secondary_count);
        let name_hash = compute_name_hash(name, upcase);

        let mut stream = ExFatStreamEntry::new(first_cluster, data_len, name_length, name_hash);
        stream.general_secondary_flags |= 1; // AllocationPossible

        primary.compute_set_checksum(&stream, &names);
        Self {
            primary,
            stream,
            names,
        }
    }

    pub fn file(
        name: &str,
        cluster: u32,
        size: u32,
        attr: &FileAttributes,
        upcase: &UpcaseHandle,
    ) -> Self {
        let names = name_entries(name);
        let name_length = name_length_utf16(name);
        let secondary_count = 1 + names.len() as u8;

        let mut primary = ExFatPrimaryEntry::new(attr, secondary_count);

        let name_hash = compute_name_hash(name, upcase);

        let stream = ExFatStreamEntry::new(cluster, size as u64, name_length, name_hash);

        primary.compute_set_checksum(&stream, &names);

        Self {
            primary,
            stream,
            names,
        }
    }

    pub fn file_contiguous(
        name: &str,
        cluster: u32,
        size: u32,
        attr: &FileAttributes,
        upcase: &UpcaseHandle,
    ) -> Self {
        let names = name_entries(name);
        let name_length = name_length_utf16(name);
        let secondary_count = 1 + names.len() as u8;

        let mut primary = ExFatPrimaryEntry::new(attr, secondary_count);

        let name_hash = compute_name_hash(name, upcase);

        let mut stream = ExFatStreamEntry::new(cluster, size as u64, name_length, name_hash);
        stream.general_secondary_flags |= 0x02; // NoFatChain

        primary.compute_set_checksum(&stream, &names);

        Self {
            primary,
            stream,
            names,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        self.primary.to_raw_buffer(buf);

        self.stream.to_raw_buffer(buf);

        for name in &self.names {
            name.to_raw_buffer(buf);
        }
    }

    pub fn from_raw(
        raw_name_stack: &[[u8; 32]],
        raw_primary: &[u8],
        raw_stream: &[u8],
    ) -> FsParsingResult<Self> {
        if raw_primary.len() != 32 || raw_stream.len() != 32 {
            return Err(FsParsingError::Invalid(
                "Primary or Stream Entry invalid size",
            ));
        }

        let primary = ExFatPrimaryEntry::read_from_bytes(raw_primary)
            .map_err(|_| FsParsingError::Invalid("Invalid Primary Entry"))?;

        let stream = ExFatStreamEntry::read_from_bytes(raw_stream)
            .map_err(|_| FsParsingError::Invalid("Invalid Stream Entry"))?;

        let mut names = Vec::with_capacity(raw_name_stack.len());
        for lfn in raw_name_stack.iter() {
            if lfn.len() != 32 {
                return Err(FsParsingError::Invalid("Invalid Name Entry size"));
            }

            let name = ExFatNameEntry::read_from_bytes(lfn)
                .map_err(|_| FsParsingError::Invalid("Invalid Name Entry"))?;

            names.push(name);
        }

        Ok(Self {
            primary,
            stream,
            names,
        })
    }
}

fn name_entries(name: &str) -> Vec<ExFatNameEntry> {
    let name_utf16: Vec<u16> = name.encode_utf16().collect();
    let count = name_utf16.len().div_ceil(15);

    (0..count)
        .map(|i| {
            let start = i * 15;
            let end = ((i + 1) * 15).min(name_utf16.len());

            let mut name_chars = [0x0000u16; 15];
            for (j, &c) in name_utf16[start..end].iter().enumerate() {
                name_chars[j] = c;
            }

            ExFatNameEntry::new(name_chars)
        })
        .collect()
}

fn decode_name(names: &[ExFatNameEntry]) -> FsParsingResult<String> {
    let mut name_utf16 = Vec::with_capacity(names.len() * 15);
    for name_entry in names {
        let name_chars = name_entry.name_chars;
        for &c in name_chars.iter() {
            if c == 0x0000 || c == 0xFFFF {
                break;
            }
            name_utf16.push(c);
        }
    }
    String::from_utf16(&name_utf16)
        .map_err(|_| FsParsingError::Invalid("Invalid UTF-16 in ExFat name"))
}

#[inline]
fn compute_name_hash(name: &str, upcase: &UpcaseHandle) -> u16 {
    let mut h: u16 = 0;
    for cu in name.encode_utf16() {
        let b = upcase.upper(cu).to_le_bytes();
        h = h.rotate_right(1).wrapping_add(b[0] as u16);
        h = h.rotate_right(1).wrapping_add(b[1] as u16);
    }
    h
}

#[inline]
fn name_length_utf16(name: &str) -> u8 {
    let n = name.encode_utf16().count();
    u8::try_from(n).expect("exFAT supports up to 255 UTF-16 units")
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatPrimaryEntry {
    pub entry_type: u8,
    pub secondary_count: u8,
    pub set_checksum: u16,
    pub file_attributes: u16,
    pub reserved1: u16,
    pub create_timestamp: u32,
    pub modify_timestamp: u32,
    pub access_timestamp: u32,
    pub create_10ms_increment: u8,
    pub modify_10ms_increment: u8,
    pub create_utc_offset: u8,
    pub modify_utc_offset: u8,
    pub access_utc_offset: u8,
    pub reserved2: [u8; 7],
}

impl ExFatPrimaryEntry {
    pub fn new(attr: &FileAttributes, secondary_count: u8) -> Self {
        let created = attr.created.unwrap_or_else(time_utils::now_utc);
        let modified = attr.modified.unwrap_or(created);
        let accessed = attr.accessed.unwrap_or(modified);

        let (c_time, c_fine, c_utc) = utils::datetime_from(created);
        let (m_time, m_fine, m_utc) = utils::datetime_from(modified);
        let (a_time, _, a_utc) = utils::datetime_from(accessed);

        Self {
            entry_type: EXFAT_ENTRY_PRIMARY,
            secondary_count,
            set_checksum: 0, // computed later
            file_attributes: attr.as_exfat_attr(),
            reserved1: 0,
            create_timestamp: c_time,
            modify_timestamp: m_time,
            access_timestamp: a_time,
            create_10ms_increment: c_fine,
            modify_10ms_increment: m_fine,
            create_utc_offset: c_utc,
            modify_utc_offset: m_utc,
            access_utc_offset: a_utc,
            reserved2: [0u8; 7],
        }
    }

    pub fn compute_set_checksum(&mut self, stream: &ExFatStreamEntry, names: &[ExFatNameEntry]) {
        let mut sum = 0u16;

        // Primary (ignorer SetChecksum aux offsets 2..=3)
        let p = self.as_bytes();
        for (i, &b) in p.iter().enumerate() {
            if i == 2 || i == 3 {
                continue;
            }
            sum = sum.rotate_right(1).wrapping_add(b as u16);
        }

        // Stream
        let s = stream.as_bytes();
        for &b in s.iter() {
            sum = sum.rotate_right(1).wrapping_add(b as u16);
        }

        // FileName entries
        for name in names {
            for &b in name.as_bytes().iter() {
                sum = sum.rotate_right(1).wrapping_add(b as u16);
            }
        }

        self.set_checksum = sum;
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatStreamEntry {
    pub entry_type: u8,
    pub general_secondary_flags: u8,
    pub reserved1: u8,
    pub name_length: u8,
    pub name_hash: u16,
    pub reserved2: u16,
    pub valid_data_length: u64,
    pub reserved3: u32,
    pub first_cluster: u32,
    pub data_length: u64,
}

impl ExFatStreamEntry {
    pub fn new(first_cluster: u32, data_length: u64, name_length: u8, name_hash: u16) -> Self {
        Self {
            entry_type: EXFAT_ENTRY_STREAM,
            general_secondary_flags: 0,
            reserved1: 0,
            name_length,
            name_hash,
            reserved2: 0,
            valid_data_length: data_length,
            reserved3: 0,
            first_cluster,
            data_length,
        }
    }

    /// Flag NoFatChain (contiguous) : bit1 de `general_secondary_flags`
    #[inline]
    pub fn is_contiguous(&self) -> bool {
        (self.general_secondary_flags & 0x02) != 0
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct ExFatNameEntry {
    pub entry_type: u8,
    pub reserved: u8,
    pub name_chars: [u16; 15],
}

impl ExFatNameEntry {
    pub fn new(name_chars: [u16; EXFAT_NAME_ENTRY_CHARS]) -> Self {
        Self {
            entry_type: EXFAT_ENTRY_NAME,
            reserved: 0,
            name_chars,
        }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct ExFatEodEntry {
    pub entry_type: u8,
    pub reserved: [u8; 31],
}

impl ExFatEodEntry {
    pub fn new() -> Self {
        Self {
            entry_type: EXFAT_EOD,
            reserved: [0u8; 31],
        }
    }

    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}
