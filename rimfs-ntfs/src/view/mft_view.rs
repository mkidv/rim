// SPDX-License-Identifier: MIT
//! MFT record view + attribute iterator.

use crate::types::*;
use zerocopy::FromBytes;

use super::attr_view::AttrRef;

#[derive(Debug, Clone, Copy)]
pub struct MftRecordView<'a> {
    buf: &'a [u8],
    header: &'a MftRecordHeader,
}

impl<'a> MftRecordView<'a> {
    pub fn new(buf: &'a [u8]) -> Result<Self, MftViewError> {
        let (hdr, _) = MftRecordHeader::ref_from_prefix(buf)
            .map_err(|_| MftViewError::Malformed("mft header"))?;
        if !hdr.is_file_record() {
            return Err(MftViewError::Malformed("bad signature"));
        }
        Ok(Self { buf, header: hdr })
    }

    pub fn header(&self) -> &MftRecordHeader {
        self.header
    }

    pub fn is_dir(&self) -> bool {
        self.header.is_dir()
    }

    pub fn attrs(&self) -> AttrIter<'a> {
        AttrIter {
            record: self.buf,
            offset: self.header.attrs_offset.get() as usize,
            done: false,
        }
    }

    pub fn find(&self, attr_type: NtfsAttributeType) -> Result<Option<AttrRef<'a>>, MftViewError> {
        self.find_named(attr_type, None)
    }

    #[cfg(feature = "alloc")]
    pub fn find_named(
        &self,
        attr_type: NtfsAttributeType,
        stream_name: Option<&str>,
    ) -> Result<Option<AttrRef<'a>>, MftViewError> {
        for a in self.attrs() {
            let a = a?;
            if a.ty() == attr_type.code() {
                match stream_name {
                    None => {
                        if a.header.name_length == 0 {
                            return Ok(Some(a));
                        }
                    }
                    Some(target_name) => {
                        if a.name_eq_ignore_ascii_case(target_name) {
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

        let Some(slice) = self.record.get(self.offset..) else {
            self.done = true;
            return Some(Err(MftViewError::OutOfBounds));
        };
        // The end marker occupies only four bytes, not a full attribute header.
        let Ok((kind, _)) = zerocopy::byteorder::little_endian::U32::ref_from_prefix(slice) else {
            self.done = true;
            return Some(Err(MftViewError::Malformed("missing attribute end marker")));
        };
        if kind.get() == NtfsAttributeType::End.code() {
            self.done = true;
            return None;
        }

        let (hdr, _) = match AttributeHeader::ref_from_prefix(slice) {
            Ok(x) => x,
            Err(_) => {
                self.done = true;
                return Some(Err(MftViewError::Malformed("attr header")));
            }
        };

        let total = hdr.length.get() as usize;
        if total < core::mem::size_of::<AttributeHeader>() || total > slice.len() {
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

#[cfg(test)]
mod iterator_tests {
    use super::*;
    #[test]
    fn short_end_marker_is_valid_but_truncated_header_is_not() {
        for len in 0..16 {
            let bytes = [0u8; 16];
            let mut iter = AttrIter {
                record: &bytes[..len],
                offset: 0,
                done: false,
            };
            assert!(iter.next().unwrap().is_err());
            assert!(iter.next().is_none());
        }
        let marker = NtfsAttributeType::End.code().to_le_bytes();
        let mut iter = AttrIter {
            record: &marker,
            offset: 0,
            done: false,
        };
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }
}
