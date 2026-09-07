// SPDX-License-Identifier: MIT
//! NTFS filename, DOS 8.3 generation, and UpCase name comparison utilities.

use core::cmp::Ordering;

use crate::upcase::UpcaseHandle;

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{format, string::String};

/// Determine the NTFS filename namespace for an entry.
///
/// If the name fits DOS 8.3 constraints, use `Win32AndDos` (3).
/// Otherwise, use `Win32` (1) accompanied by an alias in `Dos` (2).
pub fn determine_file_name_namespace(name: &str) -> crate::types::record::NtfsFileNameNamespace {
    if is_valid_dos_8_3(name) {
        crate::types::record::NtfsFileNameNamespace::Win32AndDos
    } else {
        crate::types::record::NtfsFileNameNamespace::Win32
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
