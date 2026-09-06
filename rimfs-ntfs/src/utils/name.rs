// SPDX-License-Identifier: MIT
//! NTFS filename, DOS 8.3 generation, and UpCase name comparison utilities.

use core::cmp::Ordering;

use crate::upcase::UpcaseHandle;

/// Determine the NTFS filename namespace based on whether the name fits DOS 8.3 constraints.
/// Names fitting DOS 8.3 use Win32AndDos; other names (long, unicode) use Win32.
pub fn determine_file_name_namespace(name: &str) -> crate::types::record::NtfsFileNameNamespace {
    if is_valid_dos_8_3(name) {
        crate::types::record::NtfsFileNameNamespace::Win32AndDos
    } else {
        crate::types::record::NtfsFileNameNamespace::Win32
    }
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
