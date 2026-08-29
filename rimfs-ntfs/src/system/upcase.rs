// SPDX-License-Identifier: MIT
//! NTFS $UpCase handling (Windows const or ntfs-3g legacy generated)
//
// no_std + alloc friendly

use crate::constant::upcase::UPCASE_TABLE;
use crate::core::utils::upcase::UpcaseHandle as CoreUpcase;

#[derive(Debug, Clone, PartialEq)]
pub enum UpcaseFlavor {
    /// Windows dump
    Windows,
    /// Generates the legacy linux-ntfs/ntfs-3g compatible table (run/dup/word tables).
    Legacy,
}

pub struct UpcaseHandle(CoreUpcase);

impl UpcaseHandle {
    pub const LEN_U16: usize = CoreUpcase::LEN_U16;
    pub const LEN_BYTES: usize = CoreUpcase::LEN_BYTES;

    #[inline]
    pub fn upper(&self, cu: u16) -> u16 {
        self.0.upper(cu)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    #[inline]
    pub fn as_u16(&self) -> &[u16] {
        self.0.as_u16()
    }

    /// Build from a known flavor (like your ExFAT `from_flavor`).
    pub fn from_flavor(flavor: &UpcaseFlavor) -> Self {
        match flavor {
            UpcaseFlavor::Windows => Self::from_windows_const(),
            UpcaseFlavor::Legacy => Self::generate_legacy(),
        }
    }

    /// Use your compile-time constant table.
    pub fn from_windows_const() -> Self {
        let handle = CoreUpcase::from_u16_slice(&UPCASE_TABLE);
        Self(handle)
    }

    /// Build from raw $UpCase bytes (must be exactly 128KiB).
    pub fn from_le_bytes(blob: &[u8]) -> Result<Self, &'static str> {
        if blob.len() != Self::LEN_BYTES {
            return Err("ntfs_upcase_bad_len");
        }
        // Core checks even length, but strict 128KB check here is NTFS specific
        let handle = CoreUpcase::from_bytes(blob).map_err(|_| "ntfs_upcase_parse_error")?;
        Ok(Self(handle))
    }

    /// Port of linux-ntfs `generate_default_upcase()` (legacy mapping set).
    pub fn generate_legacy() -> Self {
        Self(CoreUpcase::generate_legacy())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_const_tripwire() {
        let uc = UpcaseHandle::from_flavor(&UpcaseFlavor::Windows);
        assert_eq!(uc.upper(0x0061), 0x0041); // a -> A
        assert_eq!(uc.upper(0x00E9), 0x00C9); // é -> É (si ta table est Windows-like)
        assert_eq!(uc.upper(0x0430), 0x0410); // Cyrillic a -> A
        assert_eq!(uc.upper(0xFF41), 0xFF21); // fullwidth a -> A
        assert_eq!(uc.len(), UpcaseHandle::LEN_BYTES);
    }

    #[test]
    fn legacy_tripwire() {
        let uc = UpcaseHandle::from_flavor(&UpcaseFlavor::Legacy);
        assert_eq!(uc.upper(0x0061), 0x0041);
        assert_eq!(uc.upper(0x0430), 0x0410);
        assert_eq!(uc.len(), UpcaseHandle::LEN_BYTES);
    }
}
