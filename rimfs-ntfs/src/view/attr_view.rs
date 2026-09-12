// SPDX-License-Identifier: MIT
//! Attribute view (resident vs non-resident) with safe slicing.

use crate::types::*;
use zerocopy::FromBytes;

use super::runlist::NtfsRunList;

#[derive(Debug, Clone, Copy)]
pub struct AttrRef<'a> {
    /// Full attribute bytes (header..end)
    pub raw: &'a [u8],
    pub header: &'a AttributeHeader,
}

impl<'a> AttrRef<'a> {
    pub fn ty(&self) -> u32 {
        self.header.attr_type.get()
    }

    pub fn is_resident(&self) -> bool {
        self.header.is_resident()
    }

    /// Borrow the UTF-16LE name without allocating or requiring alignment.
    pub fn name_utf16(&self) -> Option<&'a [zerocopy::byteorder::little_endian::U16]> {
        let name_len = self.header.name_length as usize;
        if name_len == 0 {
            return None;
        }
        let start = self.header.name_offset.get() as usize;
        let bytes = self.raw.get(start..start.checked_add(name_len * 2)?)?;
        <[zerocopy::byteorder::little_endian::U16]>::ref_from_bytes(bytes).ok()
    }

    /// Match with the same ASCII-only case folding as `str::eq_ignore_ascii_case`.
    /// Non-ASCII characters remain case-sensitive; this does not apply $UpCase.
    pub fn name_eq_ignore_ascii_case(&self, target: &str) -> bool {
        fn fold(unit: u16) -> u16 {
            if unit >= b'A' as u16 && unit <= b'Z' as u16 {
                unit + (b'a' - b'A') as u16
            } else {
                unit
            }
        }
        self.name_utf16().is_some_and(|name| {
            name.iter()
                .map(|unit| fold(unit.get()))
                .eq(target.encode_utf16().map(fold))
        })
    }

    #[cfg(feature = "alloc")]
    pub fn name(&self) -> Option<alloc::string::String> {
        core::char::decode_utf16(self.name_utf16()?.iter().map(|unit| unit.get()))
            .collect::<Result<alloc::string::String, _>>()
            .ok()
    }

    pub fn as_view(&self) -> Result<AttrView<'a>, AttrViewError> {
        if self.is_resident() {
            let res_off = core::mem::size_of::<AttributeHeader>();
            let (res, _) = ResidentAttributeHeader::ref_from_prefix(
                self.raw.get(res_off..).ok_or(AttrViewError::OutOfBounds)?,
            )
            .map_err(|_| AttrViewError::Malformed("resident header"))?;

            let start = res.value_offset.get() as usize;
            let end = start
                .checked_add(res.value_length.get() as usize)
                .ok_or(AttrViewError::OutOfBounds)?;

            if end > self.raw.len() {
                return Err(AttrViewError::OutOfBounds);
            }

            Ok(AttrView::Resident {
                value: &self.raw[start..end],
                value_len: res.value_length.get(),
            })
        } else {
            let nr_off = core::mem::size_of::<AttributeHeader>();
            let (nr, _) = NonResidentAttributeHeader::ref_from_prefix(
                self.raw.get(nr_off..).ok_or(AttrViewError::OutOfBounds)?,
            )
            .map_err(|_| AttrViewError::Malformed("nonresident header"))?;

            let runs_off = nr.data_runs_offset.get() as usize;
            if runs_off >= self.raw.len() {
                return Err(AttrViewError::OutOfBounds);
            }

            Ok(AttrView::NonResident {
                runlist: NtfsRunList::new(&self.raw[runs_off..]),
                allocated_size: nr.allocated_size.get(),
                data_size: nr.data_size.get(),
                initialized_size: nr.initialized_size.get(),
                lowest_vcn: nr.lowest_vcn.get(),
                highest_vcn: nr.highest_vcn.get(),
            })
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AttrView<'a> {
    Resident {
        value: &'a [u8],
        value_len: u32,
    },
    NonResident {
        runlist: NtfsRunList<'a>,
        allocated_size: u64,
        data_size: u64,
        initialized_size: u64,
        lowest_vcn: u64,
        highest_vcn: u64,
    },
}

impl<'a> AttrView<'a> {
    pub fn as_resident(&self) -> Option<&'a [u8]> {
        match self {
            AttrView::Resident { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn as_nonresident(&self) -> Option<NtfsRunList<'a>> {
        match self {
            AttrView::NonResident { runlist, .. } => Some(*runlist),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum AttrViewError {
    Malformed(&'static str),
    OutOfBounds,
}

#[cfg(test)]
mod borrowed_header_tests {
    use super::*;
    #[test]
    fn borrowed_name_matching_preserves_decoding_semantics() {
        for name in ["Stream", "A\u{1f600}Z", "\u{e9}", "", "A\0B"] {
            let units: alloc::vec::Vec<u16> = name.encode_utf16().collect();
            let mut bytes = alloc::vec![0u8; 17 + units.len() * 2];
            bytes[9] = units.len() as u8;
            bytes[10..12].copy_from_slice(&17u16.to_le_bytes());
            for (slot, unit) in bytes[17..].chunks_exact_mut(2).zip(units) {
                slot.copy_from_slice(&unit.to_le_bytes());
            }
            let header = AttributeHeader::ref_from_prefix(&bytes).unwrap().0;
            let attr = AttrRef {
                raw: &bytes,
                header,
            };
            for target in [
                name,
                "stream",
                "STREAM",
                "A\u{1f600}z",
                "\u{c9}",
                "",
                "A\0b",
            ] {
                assert_eq!(
                    attr.name_eq_ignore_ascii_case(target),
                    attr.name().is_some_and(|n| n.eq_ignore_ascii_case(target))
                );
            }
        }
        let mut bytes = [0u8; 19];
        bytes[9] = 1;
        bytes[10] = 17;
        bytes[17..].copy_from_slice(&0xd800u16.to_le_bytes());
        let header = AttributeHeader::ref_from_prefix(&bytes).unwrap().0;
        let attr = AttrRef {
            raw: &bytes,
            header,
        };
        assert!(attr.name().is_none());
        assert!(!attr.name_eq_ignore_ascii_case("\u{fffd}"));
        let truncated = AttrRef {
            raw: &bytes[..18],
            header,
        };
        assert!(truncated.name_utf16().is_none());
    }

    #[test]
    fn truncated_attribute_tail_returns_error() {
        let mut bytes = [0u8; 16];
        for non_resident in [0, 1] {
            bytes[8] = non_resident;
            let header = AttributeHeader::read_from_bytes(&bytes).unwrap();
            let view = AttrRef {
                raw: &[],
                header: &header,
            };
            assert!(matches!(view.as_view(), Err(AttrViewError::OutOfBounds)));
        }
    }
}
