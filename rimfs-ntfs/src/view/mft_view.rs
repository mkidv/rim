// SPDX-License-Identifier: MIT
//! MFT record view + attribute iterator.

use crate::{constant::ATTR_END, types::*};
use zerocopy::FromBytes;

use super::attr_view::AttrRef;

#[derive(Debug, Clone, Copy)]
pub struct MftRecordView<'a> {
    buf: &'a [u8],
    header: MftRecordHeader,
}

impl<'a> MftRecordView<'a> {
    pub fn new(buf: &'a [u8]) -> Result<Self, MftViewError> {
        let (hdr, _) = MftRecordHeader::read_from_prefix(buf)
            .map_err(|_| MftViewError::Malformed("mft header"))?;
        if !hdr.is_file_record() {
            return Err(MftViewError::Malformed("bad signature"));
        }
        Ok(Self { buf, header: hdr })
    }

    pub fn header(&self) -> &MftRecordHeader {
        &self.header
    }

    pub fn is_dir(&self) -> bool {
        self.header.is_dir()
    }

    pub fn attrs(&self) -> AttrIter<'a> {
        AttrIter {
            record: self.buf,
            offset: self.header.attrs_offset as usize,
            done: false,
        }
    }

    pub fn find(&self, attr_type: u32) -> Result<Option<AttrRef<'a>>, MftViewError> {
        self.find_named(attr_type, None)
    }

    #[cfg(feature = "alloc")]
    pub fn find_named(
        &self,
        attr_type: u32,
        stream_name: Option<&str>,
    ) -> Result<Option<AttrRef<'a>>, MftViewError> {
        for a in self.attrs() {
            let a = a?;
            if a.ty() == attr_type {
                match stream_name {
                    None => {
                        if a.header.name_length == 0 {
                            return Ok(Some(a));
                        }
                    }
                    Some(target_name) => {
                        if a.name()
                            .is_some_and(|n| n.eq_ignore_ascii_case(target_name))
                        {
                            return Ok(Some(a));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

pub struct AttrIter<'a> {
    record: &'a [u8],
    offset: usize,
    done: bool,
}

impl<'a> Iterator for AttrIter<'a> {
    type Item = Result<AttrRef<'a>, MftViewError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        if self.offset + core::mem::size_of::<AttributeHeader>() > self.record.len() {
            self.done = true;
            return None;
        }

        let slice = &self.record[self.offset..];
        let (hdr, _) = match AttributeHeader::read_from_prefix(slice) {
            Ok(x) => x,
            Err(_) => {
                self.done = true;
                return Some(Err(MftViewError::Malformed("attr header")));
            }
        };

        if hdr.attr_type == ATTR_END {
            self.done = true;
            return None;
        }

        let total = hdr.length as usize;
        if total == 0 || self.offset + total > self.record.len() {
            self.done = true;
            return Some(Err(MftViewError::OutOfBounds));
        }

        let raw = &self.record[self.offset..self.offset + total];
        self.offset += total;

        Some(Ok(AttrRef { raw, header: hdr }))
    }
}

#[derive(Debug)]
pub enum MftViewError {
    Malformed(&'static str),
    OutOfBounds,
}
