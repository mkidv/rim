#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{string::String, vec, vec::Vec};

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    FsMeta, Validate,
    core::{errors::*, resolver::*},
    {
        attr::FatFileAttributesExt,
        attr::*,
        constant::{FAT_DOT_NAME, FAT_DOTDOT_NAME, FAT_EOD, FAT_FIRST_CLUSTER},
        meta::*,
        utils,
    },
};

#[derive(Debug, Clone)]
pub struct FatEntries {
    pub lfn: Vec<FatLFNEntry>,
    pub entry: FatEntry,
    pub contiguous_hint: bool,
}

impl FatEntries {
    /// Unified accessor to decoded name
    pub fn name(&self) -> FsParsingResult<String> {
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
        let mut attr = FileAttributes::from_fat_attr(self.entry.attr);
        attr.contiguous = self.contiguous_hint;
        attr
    }

    pub fn is_dir(&self) -> bool {
        self.entry.attr & FatAttributes::DIRECTORY.bits() != 0
    }

    pub fn first_cluster(&self) -> u32 {
        self.entry.first_cluster()
    }

    pub fn contiguous_hint(&self) -> bool {
        self.contiguous_hint
    }

    pub fn dir(name: &str, cluster: u32, attr: &FileAttributes) -> Self {
        let (date, time, fine) = utils::datetime_from_attr(attr);
        let (short_name, is_lfn) = utils::to_short_name(name);
        let lfn = if is_lfn {
            utils::lfn_entries(name, &short_name)
        } else {
            vec![]
        };
        let entry = FatEntry::new(
            short_name,
            FatAttributes::DIRECTORY.bits(),
            cluster,
            0,
            date,
            time,
            fine,
        );
        Self {
            lfn,
            entry,
            contiguous_hint: false,
        }
    }

    pub fn file(name: &str, cluster: u32, size: u32, attr: &FileAttributes) -> Self {
        let (date, time, fine) = utils::datetime_from_attr(attr);
        let (short_name, is_lfn) = utils::to_short_name(name);
        let lfn = if is_lfn {
            utils::lfn_entries(name, &short_name)
        } else {
            vec![]
        };
        let entry = FatEntry::new(
            short_name,
            attr.as_fat_attr(),
            cluster,
            size,
            date,
            time,
            fine,
        );
        Self {
            lfn,
            entry,
            contiguous_hint: attr.contiguous,
        }
    }

    pub fn volume_label(name: [u8; 11]) -> Self {
        let entry = FatEntry::new(name, FatAttributes::VOLUME_ID.bits(), 0, 0, 0, 0, 0);
        Self {
            lfn: vec![],
            entry,
            contiguous_hint: false,
        }
    }

    pub fn dot(current_cluster: u32, attr: &FileAttributes) -> Self {
        let (date, time, fine) = utils::datetime_from_attr(attr);
        let entry = FatEntry::new(
            *FAT_DOT_NAME,
            FatAttributes::DIRECTORY.bits(),
            current_cluster,
            0,
            date,
            time,
            fine,
        );
        Self {
            lfn: vec![],
            entry,
            contiguous_hint: false,
        }
    }

    pub fn dotdot(parent_cluster: u32, attr: &FileAttributes) -> Self {
        let (date, time, fine) = utils::datetime_from_attr(attr);
        let entry = FatEntry::new(
            *FAT_DOTDOT_NAME,
            FatAttributes::DIRECTORY.bits(),
            parent_cluster,
            0,
            date,
            time,
            fine,
        );
        Self {
            lfn: vec![],
            entry,
            contiguous_hint: false,
        }
    }

    #[inline(always)]
    pub fn with_integrity_calculated(mut self, meta: &FatMeta) -> Self {
        if meta.use_integrity {
            self.entry.update_crc(&self.lfn, self.contiguous_hint);
        }
        self
    }

    pub fn with_contiguous_hint(mut self, contiguous: bool) -> Self {
        self.contiguous_hint = contiguous;
        self
    }

    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        for lfn in &self.lfn {
            lfn.to_raw_buffer(buf);
        }
        self.entry.to_raw_buffer(buf);
    }

    pub fn from_raw(
        meta: &FatMeta,
        lfn_stack: &[[u8; 32]],
        raw_entry: &[u8],
    ) -> FsParsingResult<Self> {
        crate::ensure!(
            raw_entry.len() == 32,
            FsParsingError::Invalid("Invalid Dir entry")
        );
        crate::ensure!(
            raw_entry[0] != 0x00 && raw_entry[0] != 0xE5,
            FsParsingError::Invalid("Unused or deleted entry")
        );

        let entry = FatEntry::read_from_bytes(raw_entry)
            .map_err(|_| FsParsingError::Invalid("Invalid SFN entry"))?;

        entry.validate(meta)?;

        let lfn = lfn_stack
            .iter()
            .map(|bytes| {
                crate::ensure!(
                    bytes.len() == 32,
                    FsParsingError::Invalid("Invalid Name Entry size")
                );

                FatLFNEntry::read_from_bytes(bytes)
                    .map_err(|_| FsParsingError::Invalid("Invalid LFN structure"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        if meta.use_integrity {
            let full_crc = entry.calculate_crc(&lfn);
            // Design: CRC is modulo 100 [0, 99]
            // Bit 100+ is the contiguous hint
            let expected_crc = (full_crc % 100) as u8;

            // We ignore nt_reserved check because standard tools expect it to be 0
            let actual_value = entry.integrity_checksum;
            let actual_crc = actual_value % 100;

            crate::ensure!(
                expected_crc == actual_crc,
                FsParsingError::Invalid("RIM-FAT: Entry CRC mismatch")
            );

            // Valid RIM-FAT hint: only trust contiguous if CRC is valid
            let is_contiguous = actual_value >= 100;
            return Ok(Self {
                lfn,
                entry,
                contiguous_hint: is_contiguous,
            });
        }

        Ok(Self {
            lfn,
            entry,
            contiguous_hint: false,
        })
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct FatEntry {
    pub name: [u8; 11],
    pub attr: u8,
    pub nt_reserved: u8,
    pub integrity_checksum: u8, // creation_time_tenth: CRC%100 + (contiguous?100:0)
    pub creation_time: u16,
    pub creation_date: u16,
    pub access_date: u16,
    pub first_cluster_high: u16,
    pub write_time: u16,
    pub write_date: u16,
    pub first_cluster_low: u16,
    pub file_size: u32,
}

impl FatEntry {
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
            integrity_checksum: fine,
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

    pub fn calculate_crc(&self, lfns: &[FatLFNEntry]) -> u16 {
        let mut data = [0u8; 32];
        data.copy_from_slice(self.as_bytes());
        // Mask out the checksum fields (nt_reserved and integrity_checksum)
        // These are at offset 12 and 13
        data[12] = 0;
        data[13] = 0;

        let mut crc = utils::crc16(&data);

        // Include LFNs in CRC if present
        for lfn in lfns {
            crc = utils::crc16_update(crc, lfn.as_bytes());
        }

        crc
    }

    pub fn update_crc(&mut self, lfns: &[FatLFNEntry], contiguous: bool) {
        let crc = self.calculate_crc(lfns);
        // Design: CRC % 100, +100 if contiguous
        self.nt_reserved = 0;
        let base_crc = (crc % 100) as u8;
        self.integrity_checksum = base_crc + if contiguous { 100 } else { 0 };
    }
}

impl Validate<FatMeta> for FatEntry {
    type Err = FsParsingError;

    fn neutralized(&self) -> Self {
        *self
    }

    fn validate(&self, meta: &FatMeta) -> Result<(), Self::Err> {
        // Forbid empty SFN (11 x ' ') for a real entry
        crate::ensure!(
            !self.name.iter().all(|&b| b == b' '),
            FsParsingError::Invalid("SFN: empty 8.3 name")
        );
        // If first_cluster != 0, it must be within the data range
        let c = self.first_cluster();
        crate::ensure!(
            c == 0 || (c >= FAT_FIRST_CLUSTER && c <= meta.last_data_unit()),
            FsParsingError::Invalid("Entry: first_cluster out of range")
        );

        Ok(())
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug)]
#[repr(C, packed)]
pub struct FatLFNEntry {
    pub order: u8,
    pub name1: [u16; 5],
    pub attr: u8,
    pub type_field: u8,
    pub checksum: u8,
    pub name2: [u16; 6],
    pub zero: u16,
    pub name3: [u16; 2],
}

impl FatLFNEntry {
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
            attr: FatAttributes::LFN.bits(),
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
        buf.extend_from_slice(IntoBytes::as_bytes(self));
    }
}

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct FatEodEntry {
    pub marker: u8,
    pub reserved: [u8; 31],
}

impl FatEodEntry {
    pub fn new() -> Self {
        Self {
            marker: FAT_EOD,
            reserved: [0u8; 31],
        }
    }

    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(IntoBytes::as_bytes(self));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{core::utils::checksum_utils::checksum, utils::*};

    #[test]
    fn test_lfn_entry_serialization() {
        let name: Vec<u16> = "hello_world".encode_utf16().collect();
        let lfn = FatLFNEntry::new(1, true, &name, 0xAB);
        let raw = IntoBytes::as_bytes(&lfn);

        assert_eq!(raw[0] & 0x3F, 1); // Order
        assert_eq!(raw[11], 0x0F); // Attr
        assert_eq!(raw[13], 0xAB); // Checksum
    }

    /// Builds a "file" entry with LFN from a UTF-8 name.
    fn build_entries_for_name(name: &str) -> FatEntries {
        // ATTR: default to "archive" file (0x20)
        let attr = FileAttributes::new_file();

        // SFN + LFN Representation
        let (short, is_lfn) = to_short_name(name);
        let lfn = if is_lfn {
            utils::lfn_entries(name, &short)
        } else {
            vec![]
        };

        let (date, time, fine) = datetime_from_attr(&attr);
        let entry = FatEntry::new(
            short,
            attr.as_fat_attr(),
            /*cluster*/ 5,
            /*size*/ 42,
            date,
            time,
            fine,
        );

        FatEntries {
            lfn,
            entry,
            contiguous_hint: false,
        }
    }

    /// Checks that LFN order is 0x40|N, N-1, ..., 1 (on disk)
    fn assert_lfn_order_disk(lfns: &[FatLFNEntry]) {
        if lfns.is_empty() {
            return;
        }
        let first = lfns[0];
        let n = (first.order & 0x3F) as usize;
        assert_eq!(n, lfns.len(), "LFN count mismatch");
        assert!(
            first.order & 0x40 != 0,
            "First LFN must have LAST flag (0x40)"
        );

        for (i, e) in lfns.iter().enumerate() {
            let expected = (n - i) as u8;
            assert_eq!(
                e.order & 0x3F,
                expected,
                "LFN order discontinuity at index {i}"
            );
            assert_eq!(e.attr, 0x0F, "LFN attr must be 0x0F");
            assert_eq!(e.type_field, 0x00, "LFN type_field must be 0x00");
        }
    }

    /// Checks the 0x0000 terminator in the *last fragment* (the first one on disk).
    /// Applies when the total number of UTF-16 code units is NOT a multiple of 13.
    fn assert_terminator_when_applicable(name: &str, lfns: &[FatLFNEntry]) {
        if lfns.is_empty() {
            return;
        }

        let u16s: Vec<u16> = name.encode_utf16().collect();
        let rem = u16s.len() % 13;
        if rem == 0 {
            return;
        } // exactly full: no room for 0x0000, acceptable

        // On disk, the first entry (0x40|N) contains the *end* of the name.
        let first = &lfns[0];
        let frag = first.extract_utf16();
        assert_eq!(
            frag[rem], 0x0000,
            "Expected 0x0000 terminator at position {rem} in last LFN chunk"
        );
        // Positions > rem must stay 0xFFFF (padding), checking a few of them
        for (i, &v) in frag.iter().enumerate().skip(rem + 1) {
            assert_eq!(
                v, 0xFFFF,
                "Expected 0xFFFF padding after terminator at pos {i}"
            );
        }
    }

    #[test]
    fn test_sfn_only_roundtrip() {
        // ASCII 8.3 name → no LFN
        let e = build_entries_for_name("FOO.TXT");
        assert!(e.lfn.is_empty(), "SFN-only should not create LFN entries");

        let decoded = e.name().expect("decode SFN");
        assert_eq!(decoded, "foo.txt"); // decode_sfn returns lower-case on the utils side
        assert!(
            e.name_bytes_eq("Foo.TXT"),
            "ASCII case-insensitive equality for SFN"
        );
    }

    #[test]
    fn test_lfn_roundtrip_accent_emoji() {
        let candidates = [
            "Été.txt",
            "café.md",
            "snake_🐍.rs",
            "Документ.txt",
            "日本語の資料.pdf",
            "عَرَبِيّ.md",
        ];

        for name in candidates {
            let e = build_entries_for_name(name);

            // Must have LFNs (unless strict 8.3, which is not the case here)
            assert!(!e.lfn.is_empty(), "Expected LFN entries for {name}");

            // Order + flags + type
            assert_lfn_order_disk(&e.lfn);

            // Checksum consistent with SFN
            let checksum: u8 = checksum(&e.entry.name);
            assert!(
                e.lfn.iter().all(|l| l.checksum == checksum),
                "LFN checksum mismatch for {name}"
            );

            // Terminator 0x0000 (if applicable)
            assert_terminator_when_applicable(name, &e.lfn);

            // Final decoding
            let decoded = e.name().expect("decode LFN");
            assert_eq!(decoded, name, "LFN round-trip failed for {name}");

            // Strict equality for Unicode (no case-folding)
            assert!(
                e.name_bytes_eq(name),
                "Unicode strict equality should hold for {name}"
            );
        }
    }

    #[test]
    fn test_lfn_long_255_chars() {
        // 255 code units max (here we build ~255 U+0061 'a', then suffix)
        let base = "a".repeat(240);
        let name = format!("{base}_émoji_🐍.bin"); // total < 255 code units
        let e = build_entries_for_name(&name);

        assert!(!e.lfn.is_empty());
        assert_lfn_order_disk(&e.lfn);

        let decoded = e.name().expect("decode long LFN");
        assert_eq!(decoded, name);

        // If not a multiple of 13, check terminator
        assert_terminator_when_applicable(&name, &e.lfn);
    }

    #[test]
    fn test_dot_and_dotdot_serialization_layout() {
        // self = 5, parent = 2
        let self_cluster: u32 = 5;
        let parent_cluster: u32 = 2;

        // Build a minimal "directory head" buffer: '.', '..', EOD
        let mut buf = Vec::with_capacity(3 * 32);
        let dir_attr = FileAttributes::new_dir();
        FatEntries::dot(self_cluster, &dir_attr).to_raw_buffer(&mut buf); // slot 0
        FatEntries::dotdot(parent_cluster, &dir_attr).to_raw_buffer(&mut buf); // slot 1
        FatEodEntry::new().to_raw_buffer(&mut buf); // slot 2

        assert!(buf.len() >= 96, "dir head too small ({} bytes)", buf.len());

        // -------- slot 0: '.' --------
        let s0 = &buf[0..32];
        // SFN Name = ".          " (1 dot + 10 spaces)
        assert_eq!(&s0[0..11], b".          ");
        // ATTR = DIRECTORY only
        assert_eq!(s0[11], FatAttributes::DIRECTORY.bits());
        // NTRes = 0
        assert_eq!(s0[12], 0);
        // file_size = 0
        assert_eq!(u32::from_le_bytes([s0[28], s0[29], s0[30], s0[31]]), 0);
        // cluster hi/lo = self_cluster
        let hi = u16::from_le_bytes([s0[20], s0[21]]) as u32;
        let lo = u16::from_le_bytes([s0[26], s0[27]]) as u32;
        assert_eq!((hi << 16) | lo, self_cluster, "'.' cluster mismatch");

        // -------- slot 1: '..' --------
        let s1 = &buf[32..64];
        // SFN Name = "..         " (2 dots + 9 spaces)
        assert_eq!(&s1[0..11], b"..         ");
        // ATTR = DIRECTORY only
        assert_eq!(s1[11], FatAttributes::DIRECTORY.bits());
        // NTRes = 0
        assert_eq!(s1[12], 0);
        // file_size = 0
        assert_eq!(u32::from_le_bytes([s1[28], s1[29], s1[30], s1[31]]), 0);
        // cluster hi/lo = parent_cluster
        let hi = u16::from_le_bytes([s1[20], s1[21]]) as u32;
        let lo = u16::from_le_bytes([s1[26], s1[27]]) as u32;
        assert_eq!((hi << 16) | lo, parent_cluster, "'..' cluster mismatch");

        // -------- slot 2: EOD --------
        let s2 = &buf[64..96];
        assert_eq!(s2[0], FAT_EOD, "EOD marker must be 0x00");
        assert!(
            s2[1..].iter().all(|&b| b == 0),
            "EOD reserved bytes must be zero"
        );
    }

    #[test]
    fn test_eod_entry_bytes() {
        let mut v = Vec::new();
        FatEodEntry::new().to_raw_buffer(&mut v);
        assert_eq!(v.len(), 32);
        assert_eq!(v[0], FAT_EOD);
        assert!(v[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_lfn_then_sfn_serialization_order() {
        // Name that forces an LFN
        let name = "long_named_file_for_testing.txt";
        let attr = FileAttributes::new_file();
        let (date, time, fine) = datetime_from_attr(&attr);
        let (short, is_lfn) = to_short_name(name);
        assert!(is_lfn, "Expected LFN for this filename");

        // Builds entries: first LFN(s), then SFN
        let lfns = lfn_entries(name, &short);
        let entry = FatEntry::new(short, attr.as_fat_attr(), 7, 123, date, time, fine);

        // Serialization into a buffer
        let mut buf = Vec::new();
        for l in &lfns {
            l.to_raw_buffer(&mut buf);
        }
        entry.to_raw_buffer(&mut buf);

        // Checks order: nb_lfns * 32 first, then SFN
        let l = lfns.len() * 32;
        assert!(buf.len() >= l + 32);
        // The last block must be the SFN and carry the correct fields
        let sfn = &buf[l..l + 32];
        assert_ne!(sfn[11], 0x0F, "Last entry must be SFN, not LFN");
        // Consistent checksum
        let chk: u8 = checksum(&entry.name);
        for (i, lfn_raw) in buf[..l].chunks_exact(32).enumerate() {
            assert_eq!(lfn_raw[11], 0x0F, "LFN attr mismatch at #{i}");
            assert_eq!(lfn_raw[13], chk, "LFN checksum mismatch at #{i}");
        }
    }

    #[test]
    fn test_dir_entry_helpers_dir_and_file_flags() {
        let dir_attr = FileAttributes::new_dir();
        let file_attr = FileAttributes::new_file();

        let d = FatEntries::dir("DIRNAME", 12, &dir_attr);
        assert!(d.is_dir(), "dir() must set DIRECTORY flag");
        assert_eq!(d.size(), 0, "dir() size must be 0");
        assert_eq!(d.first_cluster(), 12);

        let f = FatEntries::file("file.txt", 34, 99, &file_attr);
        assert!(!f.is_dir(), "file() must not set DIRECTORY flag");
        assert_eq!(f.size(), 99);
        assert_eq!(f.first_cluster(), 34);
    }

    #[test]
    fn test_dot_dotdot_have_no_lfn_and_zero_size() {
        let dir_attr = FileAttributes::new_dir();
        let dot = FatEntries::dot(100, &dir_attr);
        let dd = FatEntries::dotdot(50, &dir_attr);

        assert!(
            dot.lfn.is_empty() && dd.lfn.is_empty(),
            "dot/dotdot must not use LFN"
        );
        let dot_file_size = dot.entry.file_size;
        let dd_file_size = dd.entry.file_size;
        assert_eq!(dot_file_size, 0);
        assert_eq!(dd_file_size, 0);
        assert_eq!(dot.entry.attr, FatAttributes::DIRECTORY.bits());
        assert_eq!(dd.entry.attr, FatAttributes::DIRECTORY.bits());
        assert_eq!(dot.first_cluster(), 100);
        assert_eq!(dd.first_cluster(), 50);
        assert_eq!(dot.entry.nt_reserved, 0);
        assert_eq!(dd.entry.nt_reserved, 0);
    }
}
