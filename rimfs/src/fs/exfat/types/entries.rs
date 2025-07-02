// SPDX-License-Identifier: MIT
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::core::parser::*;

use crate::core::utils::time_utils;
use crate::fs::exfat::{constant::*, utils};

#[derive(Debug, Clone)]
pub struct ExFatEntries {
    pub primary: ExFatPrimaryEntry,
    pub stream: ExFatStreamEntry,
    pub names: Vec<ExFatNameEntry>,
}

impl ExFatEntries {
    pub fn name(&self) -> FsParserResult<String> {
        utils::decode_name(&self.names)
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

    pub fn dir(name: &str, cluster: u32, attr: &FileAttributes) -> Self {
        let names = utils::name_entries(name);

        let name_utf16: Vec<u16> = name.encode_utf16().collect();
        let secondary_count = 1 + names.len() as u8;

        let mut primary = ExFatPrimaryEntry::new(attr, secondary_count);

        let name_hash = utils::compute_name_hash(name);

        let stream = ExFatStreamEntry::new(cluster, 0, name_utf16.len() as u8, name_hash);

        primary.compute_set_checksum(&stream, &names);

        Self {
            primary,
            stream,
            names,
        }
    }

    pub fn file(name: &str, cluster: u32, size: u32, attr: &FileAttributes) -> Self {
        let names = utils::name_entries(name);

        let name_utf16: Vec<u16> = name.encode_utf16().collect();
        let secondary_count = 1 + names.len() as u8;

        let mut primary = ExFatPrimaryEntry::new(attr, secondary_count);

        let name_hash = utils::compute_name_hash(name);

        let stream = ExFatStreamEntry::new(cluster, size as u64, name_utf16.len() as u8, name_hash);

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
        raw_name_stack: &[Vec<u8>],
        raw_primary: &[u8],
        raw_stream: &[u8],
    ) -> FsParserResult<Self> {
        if raw_primary.len() != 32 || raw_stream.len() != 32 {
            return Err(FsParserError::Invalid(
                "Primary or Stream Entry invalid size",
            ));
        }

        let primary = ExFatPrimaryEntry::read_from_bytes(raw_primary)
            .map_err(|_| FsParserError::Invalid("Invalid Primary Entry"))?;

        let stream = ExFatStreamEntry::read_from_bytes(raw_stream)
            .map_err(|_| FsParserError::Invalid("Invalid Stream Entry"))?;

        let mut names = Vec::with_capacity(raw_name_stack.len());
        for lfn in raw_name_stack.iter() {
            if lfn.len() != 32 {
                return Err(FsParserError::Invalid("Invalid Name Entry size"));
            }

            let name = ExFatNameEntry::read_from_bytes(lfn)
                .map_err(|_| FsParserError::Invalid("Invalid Name Entry"))?;

            names.push(name);
        }

        Ok(Self {
            primary,
            stream,
            names,
        })
    }
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
            set_checksum: 0, // can be computed later
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
        let arr = self.as_bytes();
        for (i, &b) in arr.iter().enumerate() {
            if i == 2 || i == 3 {
                continue;
            }
            sum = sum.wrapping_add(b as u16);
        }

        sum = stream
            .as_bytes()
            .iter()
            .fold(sum, |acc, b| acc.wrapping_add(*b as u16));

        for name in names {
            sum = name
                .as_bytes()
                .iter()
                .fold(sum, |acc, b| acc.wrapping_add(*b as u16));
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
