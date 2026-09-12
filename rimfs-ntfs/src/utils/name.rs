// SPDX-License-Identifier: MIT
//! NTFS filename, DOS 8.3 generation, and UpCase name comparison utilities.

use core::cmp::Ordering;

use crate::{types::NtfsFileNameNamespace, upcase::UpcaseHandle};

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, string::String};

/// Determine the NTFS filename namespace for an entry.
///
/// If the name fits DOS 8.3 constraints, use `Win32AndDos` (3).
/// Otherwise, use `Win32` (1) accompanied by an alias in `Dos` (2).
pub fn determine_file_name_namespace(name: &str) -> NtfsFileNameNamespace {
    if is_valid_dos_8_3(name) {
        NtfsFileNameNamespace::Win32AndDos
    } else {
        NtfsFileNameNamespace::Win32
    }
}

/// Generate a DOS 8.3 short filename alias for a long filename.
///
/// Follows Windows NTFS / DOS conventions:
/// - Strips characters illegal in DOS 8.3 (`" * + , / : ; < = > ? [ \ ] | .` and spaces)
/// - Converts lowercase ASCII to uppercase ASCII
/// - Base name: up to 6 uppercase valid characters, followed by `~1`
/// - Extension: up to 3 uppercase valid characters
pub fn generate_dos_8_3_name(name: &str) -> String {
    let (base_part, ext_part) = match name.rfind('.') {
        Some(dot_idx) if dot_idx > 0 => (&name[..dot_idx], Some(&name[dot_idx + 1..])),
        _ => (name, None),
    };

    let is_valid_dos_char =
        |b: u8| -> bool { (0x21..=0x7E).contains(&b) && !b"\"*+,/:;<=>?[\\]|.".contains(&b) };

    let mut clean_base = String::new();
    for b in base_part.bytes() {
        if is_valid_dos_char(b) {
            clean_base.push((b as char).to_ascii_uppercase());
        }
    }

    if clean_base.is_empty() {
        clean_base.push('F');
    }

    let base_truncated: String = clean_base.chars().take(6).collect();
    let mut result = format!("{}~1", base_truncated);

    if let Some(ext) = ext_part {
        let mut clean_ext = String::new();
        for b in ext.bytes() {
            if is_valid_dos_char(b) {
                clean_ext.push((b as char).to_ascii_uppercase());
            }
        }
        let ext_truncated: String = clean_ext.chars().take(3).collect();
        if !ext_truncated.is_empty() {
            result.push('.');
            result.push_str(&ext_truncated);
        }
    }

    result
}

/// Determine if a filename complies with DOS 8.3 constraints (case-preserving for Win32AndDos).
pub fn is_valid_dos_8_3(name: &str) -> bool {
    if name.is_empty() || name.len() > 12 {
        return false;
    }
    // Must be valid ASCII printable characters, no DOS-invalid symbols
    for b in name.bytes() {
        if !(0x20..=0x7E).contains(&b) || b" *+,/:;<=>?[\\]|\"".contains(&b) {
            return false;
        }
    }
    let mut parts = name.split('.');
    let base = match parts.next() {
        Some(b) => b,
        None => return false,
    };
    if base.is_empty() || base.len() > 8 {
        return false;
    }
    if let Some(ext) = parts.next() {
        if ext.len() > 3 {
            return false;
        }
        if parts.next().is_some() {
            return false; // More than 1 dot
        }
    }
    true
}

/// Compare two UTF-16 names using the NTFS Upcase table (case-insensitive)
pub fn compare_names_upcase(a: &[u16], b: &[u16], upcase: &UpcaseHandle) -> Ordering {
    let len = a.len().min(b.len());
    for i in 0..len {
        let ca = upcase.upper(a[i]);
        let cb = upcase.upper(b[i]);
        if ca != cb {
            return ca.cmp(&cb);
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn test_compare_names_upcase() {
        use crate::upcase::UpcaseFlavor;
        let upcase = UpcaseHandle::from_flavor(&UpcaseFlavor::Windows);

        let name1: Vec<u16> = "filename.txt".encode_utf16().collect();
        let name2: Vec<u16> = "FILENAME.TXT".encode_utf16().collect();
        let name3: Vec<u16> = "filename.tyt".encode_utf16().collect();

        assert_eq!(
            compare_names_upcase(&name1, &name2, &upcase),
            Ordering::Equal
        );
        assert_eq!(
            compare_names_upcase(&name1, &name3, &upcase),
            Ordering::Less
        );

        // Unicode check: Cyrillic 'a' (U+0430) and 'A' (U+0410)
        let cyr_a_lower: Vec<u16> = vec![0x0430];
        let cyr_a_upper: Vec<u16> = vec![0x0410];
        assert_eq!(
            compare_names_upcase(&cyr_a_lower, &cyr_a_upper, &upcase),
            Ordering::Equal
        );

        // Standard ASCII order check
        assert!(
            compare_names_upcase(
                &"a".encode_utf16().collect::<Vec<_>>(),
                &"B".encode_utf16().collect::<Vec<_>>(),
                &upcase
            ) == Ordering::Less
        );
    }

    #[test]
    fn test_dos_8_3_and_namespace() {
        assert!(is_valid_dos_8_3("FILE.TXT"));
        assert!(is_valid_dos_8_3("test_win.txt"));
        assert!(!is_valid_dos_8_3("win_payload"));
        assert!(!is_valid_dos_8_3("long_filename.extension"));

        assert_eq!(
            determine_file_name_namespace("FILE.TXT"),
            NtfsFileNameNamespace::Win32AndDos
        );
        assert_eq!(
            determine_file_name_namespace("win_payload"),
            NtfsFileNameNamespace::Win32
        );

        assert_eq!(generate_dos_8_3_name("win_payload"), "WIN_PA~1");
        assert_eq!(generate_dos_8_3_name("from_windows"), "FROM_W~1");
        assert_eq!(
            generate_dos_8_3_name("long_filename.extension"),
            "LONG_F~1.EXT"
        );
        assert_eq!(generate_dos_8_3_name("document.tar.gz"), "DOCUME~1.GZ");
        assert_eq!(generate_dos_8_3_name("a+b=c[1].txt"), "ABC1~1.TXT");
    }
}
