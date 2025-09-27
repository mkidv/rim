#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec;
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;

use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

use crate::{
    core::{errors::*, resolver::*},
    fs::fat32::{
        attr::*,
        constant::{FAT_DOT_NAME, FAT_DOTDOT_NAME, FAT_EOD},
        utils,
    },
};

#[derive(Debug, Clone)]
pub struct Fat32Entries {
    pub lfn: Vec<Fat32LFNEntry>,
    pub entry: Fat32Entry,
}

impl Fat32Entries {
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
        let entry = Fat32Entry::new(
            *FAT_DOT_NAME,
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
        let entry = Fat32Entry::new(
            *FAT_DOTDOT_NAME,
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

    pub fn from_raw(lfn_stack: &[[u8; 32]], raw_entry: &[u8]) -> FsParsingResult<Self> {
        if raw_entry.len() != 32 {
            return Err(FsParsingError::Invalid("Invalid Dir entry"));
        }

        if raw_entry[0] == 0x00 || raw_entry[0] == 0xE5 {
            return Err(FsParsingError::Invalid("Unused or deleted entry"));
        }

        let mut short_name = [0u8; 11];
        short_name.copy_from_slice(&raw_entry[0..11]);

        let entry = Fat32Entry::read_from_bytes(raw_entry)
            .map_err(|_| FsParsingError::Invalid("Invalid SFN entry"))?;

        let lfn = lfn_stack
            .iter()
            .map(|bytes| {
                if bytes.len() != 32 {
                    return Err(FsParsingError::Invalid("Invalid Name Entry size"));
                }

                Fat32LFNEntry::read_from_bytes(bytes)
                    .map_err(|_| FsParsingError::Invalid("Invalid LFN structure"))
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

#[derive(IntoBytes, FromBytes, KnownLayout, Immutable, Copy, Clone, Debug, Default)]
#[repr(C, packed)]
pub struct Fat32EodEntry {
    pub marker: u8,
    pub reserved: [u8; 31],
}

impl Fat32EodEntry {
    pub fn new() -> Self {
        Self {
            marker: FAT_EOD,
            reserved: [0u8; 31],
        }
    }

    pub fn to_raw_buffer(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::fat32::utils::*;

    #[test]
    fn test_lfn_entry_serialization() {
        let name: Vec<u16> = "hello_world".encode_utf16().collect();
        let lfn = Fat32LFNEntry::new(1, true, &name, 0xAB);
        let raw = lfn.as_bytes();

        assert_eq!(raw[0] & 0x3F, 1); // Order
        assert_eq!(raw[11], 0x0F); // Attr
        assert_eq!(raw[13], 0xAB); // Checksum
    }

    /// Construit une entrée "fichier" avec LFN à partir d'un nom UTF-8.
    fn build_entries_for_name(name: &str) -> Fat32Entries {
        // ATTR: fichier "archive" (0x20) par défaut
        let attr = FileAttributes::new_file();

        // Repr SFN + LFN
        let (short, is_lfn) = to_short_name(name);
        let lfn = if is_lfn {
            lfn_entries(name, &short)
        } else {
            vec![]
        };

        let (date, time, fine) = datetime_from_attr(&attr);
        let entry = Fat32Entry::new(
            short,
            attr.as_fat_attr(),
            /*cluster*/ 5,
            /*size*/ 42,
            date,
            time,
            fine,
        );

        Fat32Entries { lfn, entry }
    }

    /// Vérifie que l'ordre LFN est 0x40|N, N-1, ..., 1 (sur disque)
    fn assert_lfn_order_disk(lfns: &[Fat32LFNEntry]) {
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

    /// Vérifie le terminator 0x0000 dans le *dernier fragment* (le premier sur disque).
    /// S'applique quand le nombre total de code units UTF-16 n'est PAS un multiple de 13.
    fn assert_terminator_when_applicable(name: &str, lfns: &[Fat32LFNEntry]) {
        if lfns.is_empty() {
            return;
        }

        let u16s: Vec<u16> = name.encode_utf16().collect();
        let rem = u16s.len() % 13;
        if rem == 0 {
            return;
        } // exactement plein: pas de place pour 0x0000, acceptable

        // Sur disque, la première entrée (0x40|N) contient la *fin* du nom.
        let first = &lfns[0];
        let frag = first.extract_utf16();
        assert_eq!(
            frag[rem], 0x0000,
            "Expected 0x0000 terminator at position {rem} in last LFN chunk"
        );
        // Les positions > rem doivent rester 0xFFFF (padding), on en vérifie quelques-unes
        for i in (rem + 1)..13 {
            assert_eq!(
                frag[i], 0xFFFF,
                "Expected 0xFFFF padding after terminator at pos {i}"
            );
        }
    }

    #[test]
    fn test_sfn_only_roundtrip() {
        // Nom ASCII 8.3 → pas de LFN
        let e = build_entries_for_name("FOO.TXT");
        assert!(e.lfn.is_empty(), "SFN-only should not create LFN entries");

        let decoded = e.name().expect("decode SFN");
        assert_eq!(decoded, "foo.txt"); // decode_sfn renvoie en lower-case côté utils
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

            // Doit avoir des LFN (sauf si 8.3 strict, ce qui n'est pas le cas ici)
            assert!(!e.lfn.is_empty(), "Expected LFN entries for {name}");

            // Ordre + drapeaux + type
            assert_lfn_order_disk(&e.lfn);

            // Checksum cohérent avec le SFN
            let sum = lfn_checksum(&e.entry.name);
            assert!(
                e.lfn.iter().all(|l| l.checksum == sum),
                "LFN checksum mismatch for {name}"
            );

            // Terminator 0x0000 (si applicable)
            assert_terminator_when_applicable(name, &e.lfn);

            // Décodage final
            let decoded = e.name().expect("decode LFN");
            assert_eq!(decoded, name, "LFN round-trip failed for {name}");

            // Égalité stricte pour Unicode (pas de case-folding)
            assert!(
                e.name_bytes_eq(name),
                "Unicode strict equality should hold for {name}"
            );
        }
    }

    #[test]
    fn test_lfn_long_255_chars() {
        // 255 code units max (ici on construit ~255 U+0061 'a', puis suffixe)
        let base = core::iter::repeat('a').take(240).collect::<String>();
        let name = format!("{base}_émoji_🐍.bin"); // total < 255 code units
        let e = build_entries_for_name(&name);

        assert!(!e.lfn.is_empty());
        assert_lfn_order_disk(&e.lfn);

        let decoded = e.name().expect("decode long LFN");
        assert_eq!(decoded, name);

        // Si non multiple de 13, vérifie terminator
        assert_terminator_when_applicable(&name, &e.lfn);
    }

    #[test]
    fn test_dot_and_dotdot_serialization_layout() {
        // self = 5, parent = 2
        let self_cluster: u32 = 5;
        let parent_cluster: u32 = 2;

        // Construire un buffer "tête de répertoire" minimal: '.', '..', EOD
        let mut buf = Vec::with_capacity(3 * 32);
        Fat32Entries::dot(self_cluster).to_raw_buffer(&mut buf); // slot 0
        Fat32Entries::dotdot(parent_cluster).to_raw_buffer(&mut buf); // slot 1
        Fat32EodEntry::new().to_raw_buffer(&mut buf); // slot 2

        assert!(buf.len() >= 96, "dir head too small ({} bytes)", buf.len());

        // -------- slot 0: '.' --------
        let s0 = &buf[0..32];
        // Nom SFN = ".          " (1 point + 10 espaces)
        assert_eq!(&s0[0..11], b".          ");
        // ATTR = DIRECTORY only
        assert_eq!(s0[11], Fat32Attributes::DIRECTORY.bits());
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
        // Nom SFN = "..         " (2 points + 9 espaces)
        assert_eq!(&s1[0..11], b"..         ");
        // ATTR = DIRECTORY only
        assert_eq!(s1[11], Fat32Attributes::DIRECTORY.bits());
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
        Fat32EodEntry::new().to_raw_buffer(&mut v);
        assert_eq!(v.len(), 32);
        assert_eq!(v[0], FAT_EOD);
        assert!(v[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_lfn_then_sfn_serialization_order() {
        // Nom qui force un LFN
        let name = "long_named_file_for_testing.txt";
        let attr = FileAttributes::new_file();
        let (date, time, fine) = datetime_from_attr(&attr);
        let (short, is_lfn) = to_short_name(name);
        assert!(is_lfn, "Expected LFN for this filename");

        // Construit les entrées: d'abord LFN(s), puis SFN
        let lfns = lfn_entries(name, &short);
        let entry = Fat32Entry::new(short, attr.as_fat_attr(), 7, 123, date, time, fine);

        // Sérialisation dans un tampon
        let mut buf = Vec::new();
        for l in &lfns {
            l.to_raw_buffer(&mut buf);
        }
        entry.to_raw_buffer(&mut buf);

        // Vérifie l'ordre: nb_lfns * 32 d'abord, puis SFN
        let l = lfns.len() * 32;
        assert!(buf.len() >= l + 32);
        // Le dernier bloc doit être le SFN et porter les bons champs
        let sfn = &buf[l..l + 32];
        assert_ne!(sfn[11], 0x0F, "Last entry must be SFN, not LFN");
        // Checksum cohérent
        let chk = lfn_checksum(&entry.name);
        for (i, lfn_raw) in buf[..l].chunks_exact(32).enumerate() {
            assert_eq!(lfn_raw[11], 0x0F, "LFN attr mismatch at #{i}");
            assert_eq!(lfn_raw[13], chk, "LFN checksum mismatch at #{i}");
        }
    }

    #[test]
    fn test_dir_entry_helpers_dir_and_file_flags() {
        let dir_attr = FileAttributes::new_dir();
        let file_attr = FileAttributes::new_file();

        let d = Fat32Entries::dir("DIRNAME", 12, &dir_attr);
        assert!(d.is_dir(), "dir() must set DIRECTORY flag");
        assert_eq!(d.size(), 0, "dir() size must be 0");
        assert_eq!(d.first_cluster(), 12);

        let f = Fat32Entries::file("file.txt", 34, 99, &file_attr);
        assert!(!f.is_dir(), "file() must not set DIRECTORY flag");
        assert_eq!(f.size(), 99);
        assert_eq!(f.first_cluster(), 34);
    }

    #[test]
    fn test_dot_dotdot_have_no_lfn_and_zero_size() {
        let dot = Fat32Entries::dot(100);
        let dd = Fat32Entries::dotdot(50);

        assert!(
            dot.lfn.is_empty() && dd.lfn.is_empty(),
            "dot/dotdot must not use LFN"
        );
        let dot_file_size = dot.entry.file_size;
        let dd_file_size = dd.entry.file_size;
        assert_eq!(dot_file_size, 0);
        assert_eq!(dd_file_size, 0);
        assert_eq!(dot.entry.attr, Fat32Attributes::DIRECTORY.bits());
        assert_eq!(dd.entry.attr, Fat32Attributes::DIRECTORY.bits());
        assert_eq!(dot.first_cluster(), 100);
        assert_eq!(dd.first_cluster(), 50);
        assert_eq!(dot.entry.nt_reserved, 0);
        assert_eq!(dd.entry.nt_reserved, 0);
    }
}
