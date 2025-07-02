#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::core::parser::*;
use crate::fs::fat32::{attr::*, utils};

#[derive(Debug, Clone)]
pub struct Fat32Entries {
    pub lfn: Vec<Fat32LFNEntry>,
    pub entry: Fat32Entry,
}

impl Fat32Entries {
    /// Unified accessor to decoded name
    pub fn name(&self) -> FsParserResult<String> {
        if self.lfn.is_empty() {
            utils::decode_sfn(&self.entry.name)
        } else {
            utils::decode_lfn(&self.lfn)
        }
    }

    pub fn name_bytes_eq(&self, target: &str) -> bool {
        if let Ok(name) = self.name() {
            name.eq_ignore_ascii_case(target)
        } else {
            false
        }
    }

    pub fn size(&self) -> usize {
        self.entry.file_size as usize
    }

    pub fn attr(&self) -> FileAttributes {
        FileAttributes::from_fat_attr(self.entry.attr)
    }

    pub fn is_dir(&self) -> bool {
        self.entry.attr & Fat32Attributes::DIRECTORY.bits() != 0
    }

    pub fn first_cluster(&self) -> u32 {
        self.entry.first_cluster()
    }

    pub fn dir(name: &str, cluster: u32, attr: &FileAttributes) -> Self {
        let (date, time, fine) = utils::datetime_from_attr(attr);
        let (short_name, is_lfn) = utils::to_short_name(name);
        let lfn = if is_lfn {
            utils::lfn_entries(name, &short_name)
        } else {
            vec![]
        };
        let entry = Fat32Entry::new(
            short_name,
            Fat32Attributes::DIRECTORY.bits(),
            cluster,
            0,
            date,
            time,
            fine,
        );
        Self { lfn, entry }
    }

    pub fn file(name: &str, cluster: u32, size: u32, attr: &FileAttributes) -> Self {
        let (date, time, fine) = utils::datetime_from_attr(attr);
        let (short_name, is_lfn) = utils::to_short_name(name);
        let lfn = if is_lfn {
            utils::lfn_entries(name, &short_name)
        } else {
            vec![]
        };
        let entry = Fat32Entry::new(
            short_name,
            attr.as_fat_attr(),
            cluster,
            size,
            date,
            time,
            fine,
        );
        Self { lfn, entry }
    }

    pub fn volume_label(name: [u8; 11]) -> Self {
        let (date, time, fine) = utils::datetime_now();
        let entry = Fat32Entry::new(
            name,
            Fat32Attributes::VOLUME_ID.bits(),
            0,
            0,
            date,
            time,
            fine,
        );
        Self { lfn: vec![], entry }
    }

    pub fn dot(current_cluster: u32) -> Self {
        let (date, time, fine) = utils::datetime_now();
        let mut name = [b' '; 11];
        name[0] = b'.';
        let entry = Fat32Entry::new(
            name,
            Fat32Attributes::DIRECTORY.bits(),
            current_cluster,
            0,
            date,
            time,
            fine,
        );
        Self { lfn: vec![], entry }
    }

    pub fn dotdot(parent_cluster: u32) -> Self {
        let (date, time, fine) = utils::datetime_now();
        let mut name = [b' '; 11];
        name[0..2].copy_from_slice(b"..");
        let entry = Fat32Entry::new(
            name,
            Fat32Attributes::DIRECTORY.bits(),
            parent_cluster,
            0,
            date,
            time,
            fine,
        );
        Self { lfn: vec![], entry }
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        for lfn in &self.lfn {
            lfn.to_raw_buffer(buf);
        }
        self.entry.to_raw_buffer(buf);
    }

    pub fn from_raw(lfn_stack: &[[u8; 32]], raw_entry: &[u8]) -> FsParserResult<Self> {
        if raw_entry.len() < 32 {
            return Err(FsParserError::Invalid("Invalid Dir entry"));
        }

        if raw_entry[0] == 0x00 || raw_entry[0] == 0xE5 {
            return Err(FsParserError::Invalid("Unused or deleted entry"));
        }

        let mut short_name = [0u8; 11];
        short_name.copy_from_slice(&raw_entry[0..11]);

        let entry = Fat32Entry::read_from_bytes(raw_entry)
            .map_err(|_| FsParserError::Invalid("Invalid SFN entry"))?;

        let lfn = lfn_stack
            .iter()
            .map(|bytes| {
                Fat32LFNEntry::read_from_bytes(bytes)
                    .map_err(|_| FsParserError::Invalid("Invalid LFN structure"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self { lfn, entry })
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct Fat32Entry {
    pub name: [u8; 11],
    pub attr: u8,
    pub nt_reserved: u8,
    pub creation_time_tenth: u8,
    pub creation_time: u16,
    pub creation_date: u16,
    pub access_date: u16,
    pub first_cluster_high: u16,
    pub write_time: u16,
    pub write_date: u16,
    pub first_cluster_low: u16,
    pub file_size: u32,
}

impl Fat32Entry {
    pub fn new(
        name: [u8; 11],
        attr: u8,
        cluster: u32,
        size: u32,
        date: u16,
        time: u16,
        fine: u8,
    ) -> Self {
        let high = ((cluster >> 16) & 0xFFFF) as u16;
        let low = (cluster & 0xFFFF) as u16;
        Self {
            name,
            attr,
            nt_reserved: 0,
            creation_time_tenth: fine,
            creation_time: time,
            creation_date: date,
            access_date: date,
            first_cluster_high: high,
            write_time: time,
            write_date: date,
            first_cluster_low: low,
            file_size: size,
        }
    }

    pub fn first_cluster(&self) -> u32 {
        ((self.first_cluster_high as u32) << 16) | (self.first_cluster_low as u32)
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct Fat32LFNEntry {
    pub order: u8,
    pub name1: [u16; 5],
    pub attr: u8,
    pub type_field: u8,
    pub checksum: u8,
    pub name2: [u16; 6],
    pub zero: u16,
    pub name3: [u16; 2],
}

impl Fat32LFNEntry {
    pub fn new(
        order: u8,
        is_last: bool,
        name_chunk: &[u16], // max 13
        checksum: u8,
    ) -> Self {
        let mut name1 = [0xFFFFu16; 5];
        let mut name2 = [0xFFFFu16; 6];
        let mut name3 = [0xFFFFu16; 2];

        // Fill unicode name chunk
        for (i, &c) in name_chunk.iter().enumerate() {
            match i {
                0..=4 => name1[i] = c,
                5..=10 => name2[i - 5] = c,
                11..=12 => name3[i - 11] = c,
                _ => break,
            }
        }

        Self {
            order: if is_last { order | 0x40 } else { order },
            name1,
            attr: Fat32Attributes::LFN.bits(),
            type_field: 0x00,
            checksum,
            name2,
            zero: 0,
            name3,
        }
    }

    pub fn extract_utf16(&self) -> [u16; 13] {
        let mut out = [0xFFFFu16; 13];
        let name1 = self.name1;
        let name2 = self.name2;
        let name3 = self.name3;
        out[0..5].copy_from_slice(&name1);
        out[5..11].copy_from_slice(&name2);
        out[11..13].copy_from_slice(&name3);
        out
    }

    #[inline(always)]
    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lfn_entry_serialization() {
        let name: Vec<u16> = "hello_world".encode_utf16().collect();
        let lfn = Fat32LFNEntry::new(1, true, &name, 0xAB);
        let raw = lfn.as_bytes();

        assert_eq!(raw[0] & 0x3F, 1); // Order
        assert_eq!(raw[11], 0x0F); // Attr
        assert_eq!(raw[13], 0xAB); // Checksum
    }
}
