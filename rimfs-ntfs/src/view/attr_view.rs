// SPDX-License-Identifier: MIT
//! Attribute view (resident vs non-resident) with safe slicing.

use crate::types::*;
use zerocopy::FromBytes;

use super::runlist::NtfsRunList;

#[derive(Debug, Clone, Copy)]
pub struct AttrRef<'a> {
    /// Full attribute bytes (header..end)
    pub raw: &'a [u8],
    pub header: AttributeHeader,
}

impl<'a> AttrRef<'a> {
    pub fn ty(&self) -> u32 {
        self.header.attr_type
    }

    pub fn is_resident(&self) -> bool {
        self.header.is_resident()
    }

    #[cfg(feature = "alloc")]
    pub fn name(&self) -> Option<alloc::string::String> {
        let name_len = self.header.name_length as usize;
        if name_len == 0 {
            return None;
        }
        let name_off = self.header.name_offset as usize;
        if name_off + name_len * 2 > self.raw.len() {
            return None;
        }
        let name_bytes = &self.raw[name_off..name_off + name_len * 2];
        let mut u16_chars = alloc::vec::Vec::with_capacity(name_len);
        for chunk in name_bytes.chunks_exact(2) {
            u16_chars.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        alloc::string::String::from_utf16(&u16_chars).ok()
    }

    pub fn as_view(&self) -> Result<AttrView<'a>, AttrViewError> {
        if self.is_resident() {
            let res_off = core::mem::size_of::<AttributeHeader>();
            let (res, _) = ResidentAttributeHeader::read_from_prefix(&self.raw[res_off..])
                .map_err(|_| AttrViewError::Malformed("resident header"))?;

            let start = res.value_offset as usize;
            let end = start
                .checked_add(res.value_length as usize)
                .ok_or(AttrViewError::OutOfBounds)?;

            if end > self.raw.len() {
                return Err(AttrViewError::OutOfBounds);
            }

            Ok(AttrView::Resident {
                value: &self.raw[start..end],
                value_len: res.value_length,
            })
        } else {
            let nr_off = core::mem::size_of::<AttributeHeader>();
            let (nr, _) = NonResidentAttributeHeader::read_from_prefix(&self.raw[nr_off..])
                .map_err(|_| AttrViewError::Malformed("nonresident header"))?;

            let runs_off = nr.data_runs_offset as usize;
            if runs_off >= self.raw.len() {
                return Err(AttrViewError::OutOfBounds);
            }

            Ok(AttrView::NonResident {
                runlist: NtfsRunList::new(&self.raw[runs_off..]),
                allocated_size: nr.allocated_size,
                data_size: nr.data_size,
                initialized_size: nr.initialized_size,
                lowest_vcn: nr.lowest_vcn,
                highest_vcn: nr.highest_vcn,
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
