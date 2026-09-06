// SPDX-License-Identifier: MIT
//! System files ($Secure, $BadClus, $Extend, $MFT::$BITMAP) integrity verification.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::format;
use rimio::RimIO;
use zerocopy::FromBytes;

use super::NtfsChecker;
use crate::constant::*;
use crate::core::bitmap::BitmapOps;
use crate::core::checker::{Finding, FsCheckerResult, VerifyReport};
use crate::flags::{MftRecordFlags, NtfsFileAttributes};
use crate::mft;
use crate::types::{
    FileNameAttribute, IndexEntryHeader, IndexNodeHeader, IndexRootHeader, StandardInformation,
};

pub struct IndexFileNameCopy {
    pub name: alloc::string::String,
    pub mft_reference: u64,
    pub file_name: FileNameAttribute,
}

impl<'a, IO: RimIO + ?Sized> NtfsChecker<'a, IO> {
    pub(crate) fn check_windows_system_invariants(
        &mut self,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        self.check_secure_invariants(rep);
        self.check_badclus_invariants(rep);
        self.check_extend_invariants(rep);
        self.check_root_i30_file_name_copies(rep);
        self.check_mft_bitmap_consistency(rep);
        Ok(())
    }

    pub(crate) fn read_record_view(&mut self, rec_num: u64) -> Option<alloc::vec::Vec<u8>> {
        mft::read_record(self.io, self.meta, rec_num).ok()
    }

    fn check_secure_invariants(&mut self, rep: &mut VerifyReport) {
        let Some(rec) = self.read_record_view(MFT_RECORD_SECURE) else {
            rep.push(Finding::err("SYS.SECURE", "$Secure record is not readable"));
            return;
        };
        let Ok(view) = crate::view::mft_view::MftRecordView::new(&rec) else {
            rep.push(Finding::err("SYS.SECURE", "$Secure record is malformed"));
            return;
        };

        let initial_errors = rep.findings.len();
        let record_flags = view.header().flags;
        if record_flags & MftRecordFlags::IS_VIEW_INDEX.bits() == 0 {
            rep.push(Finding::err(
                "SYS.SECURE.FLAGS",
                "$Secure has security indexes but MFT header lacks IS_VIEW_INDEX",
            ));
        }

        if let Some(attrs) = standard_info_attrs(&view) {
            if attrs & NtfsFileAttributes::VIEW_INDEX.bits() == 0 {
                rep.push(Finding::err(
                    "SYS.SECURE.FLAGS",
                    "$Secure $STANDARD_INFORMATION lacks VIEW_INDEX",
                ));
            }
        } else {
            rep.push(Finding::err(
                "SYS.SECURE.SI",
                "$Secure missing $STANDARD_INFORMATION",
            ));
        }

        match file_name_attr(&view) {
            Some(file_name) => {
                if file_name.file_attributes & NtfsFileAttributes::VIEW_INDEX.bits() == 0 {
                    rep.push(Finding::err(
                        "SYS.SECURE.FLAGS",
                        "$Secure $FILE_NAME lacks VIEW_INDEX",
                    ));
                }
            }
            None => rep.push(Finding::err("SYS.SECURE.FN", "$Secure missing $FILE_NAME")),
        }

        let sds = view
            .find_named(ATTR_DATA, Some("$SDS"))
            .ok()
            .flatten()
            .and_then(|attr| attr.as_view().ok());
        match sds {
            Some(crate::view::attr_view::AttrView::NonResident {
                allocated_size,
                data_size,
                initialized_size,
                ..
            }) => {
                if data_size == 0 || data_size > allocated_size || initialized_size != data_size {
                    rep.push(Finding::err(
                        "SYS.SECURE.SDS",
                        format!(
                            "$Secure:$SDS invalid sizes allocated={allocated_size} data={data_size} initialized={initialized_size}"
                        ),
                    ));
                }
            }
            _ => rep.push(Finding::err(
                "SYS.SECURE.SDS",
                "$Secure missing non-resident $DATA:$SDS",
            )),
        }

        for name in ["$SDH", "$SII"] {
            if view
                .find_named(ATTR_INDEX_ROOT, Some(name))
                .ok()
                .flatten()
                .is_none()
            {
                rep.push(Finding::err(
                    "SYS.SECURE.INDEX",
                    format!("$Secure missing $INDEX_ROOT:{name}"),
                ));
            }
        }

        if rep.findings.len() == initial_errors {
            rep.push(Finding::info(
                "SYS.SECURE",
                "$Secure Windows compatibility invariants OK",
            ));
        }
    }

    fn check_badclus_invariants(&mut self, rep: &mut VerifyReport) {
        let Some(rec) = self.read_record_view(MFT_RECORD_BADCLUS) else {
            rep.push(Finding::err(
                "SYS.BADCLUS",
                "$BadClus record is not readable",
            ));
            return;
        };
        let Ok(view) = crate::view::mft_view::MftRecordView::new(&rec) else {
            rep.push(Finding::err("SYS.BADCLUS", "$BadClus record is malformed"));
            return;
        };

        let unnamed = view
            .find_named(ATTR_DATA, None)
            .ok()
            .flatten()
            .and_then(|attr| attr.as_view().ok());
        if !matches!(
            unnamed,
            Some(crate::view::attr_view::AttrView::Resident { value, .. }) if value.is_empty()
        ) {
            rep.push(Finding::err(
                "SYS.BADCLUS.DATA",
                "$BadClus unnamed $DATA must be empty",
            ));
        }

        let bad_attr = view.find_named(ATTR_DATA, Some("$Bad")).ok().flatten();
        match bad_attr.and_then(|attr| {
            let flags = attr.header.flags;
            attr.as_view().ok().map(|view| (flags, view))
        }) {
            Some((
                _flags,
                crate::view::attr_view::AttrView::NonResident {
                    allocated_size,
                    data_size,
                    initialized_size,
                    highest_vcn,
                    runlist,
                    ..
                },
            )) => {
                let volume_size = self.meta.total_clusters * self.meta.bytes_per_cluster as u64;
                let runs: alloc::vec::Vec<_> = runlist.iter().collect();
                let sparse = runs.len() == 1
                    && runs[0].lcn.is_none()
                    && runs[0].len == self.meta.total_clusters;
                if !sparse
                    || data_size != volume_size
                    || allocated_size != volume_size
                    || initialized_size != 0
                    || highest_vcn != self.meta.total_clusters.saturating_sub(1)
                {
                    rep.push(Finding::err(
                        "SYS.BADCLUS.BAD",
                        "$BadClus:$Bad must be a sparse named stream covering the full volume",
                    ));
                } else {
                    rep.push(Finding::info(
                        "SYS.BADCLUS",
                        "$BadClus sparse $Bad stream OK",
                    ));
                }
            }
            _ => rep.push(Finding::err(
                "SYS.BADCLUS.BAD",
                "$BadClus missing non-resident sparse $DATA:$Bad",
            )),
        }
    }

    fn check_extend_invariants(&mut self, rep: &mut VerifyReport) {
        let Some(rec) = self.read_record_view(MFT_RECORD_EXTEND) else {
            rep.push(Finding::err("SYS.EXTEND", "$Extend record is not readable"));
            return;
        };
        let Ok(view) = crate::view::mft_view::MftRecordView::new(&rec) else {
            rep.push(Finding::err("SYS.EXTEND", "$Extend record is malformed"));
            return;
        };
        if !view.header().is_dir() {
            rep.push(Finding::err("SYS.EXTEND", "$Extend must be a directory"));
            return;
        }

        match resident_index_root_entries(&view, "$I30") {
            Some(entries) => {
                rep.push(Finding::info(
                    "SYS.EXTEND",
                    format!("$Extend has {} index entries", entries.len()),
                ));
            }
            None => rep.push(Finding::err(
                "SYS.EXTEND",
                "$Extend missing $INDEX_ROOT:$I30",
            )),
        }
    }

    fn check_root_i30_file_name_copies(&mut self, rep: &mut VerifyReport) {
        let Some(root_rec) = self.read_record_view(MFT_RECORD_ROOT) else {
            rep.push(Finding::err("SYS.ROOTI30", "Root record is not readable"));
            return;
        };
        let Ok(root_view) = crate::view::mft_view::MftRecordView::new(&root_rec) else {
            rep.push(Finding::err("SYS.ROOTI30", "Root record is malformed"));
            return;
        };
        let Some(entries) = resident_index_root_entries(&root_view, "$I30") else {
            return;
        };

        for entry in entries {
            let rec_num = entry.mft_reference & 0x0000_FFFF_FFFF_FFFF;
            if rec_num > MFT_RECORD_EXTEND {
                continue;
            }
            let Some(child_rec) = self.read_record_view(rec_num) else {
                rep.push(Finding::err(
                    "SYS.ROOTI30",
                    format!(
                        "Root $I30 entry {} points to unreadable record {rec_num}",
                        entry.name
                    ),
                ));
                continue;
            };
            let Ok(child_view) = crate::view::mft_view::MftRecordView::new(&child_rec) else {
                continue;
            };
            let Some(child_fn) = file_name_attr(&child_view) else {
                rep.push(Finding::err(
                    "SYS.ROOTI30",
                    format!("Record {rec_num} for {} missing $FILE_NAME", entry.name),
                ));
                continue;
            };

            if !same_file_name_metadata(&entry.file_name, &child_fn) {
                rep.push(Finding::err(
                    "SYS.ROOTI30",
                    format!(
                        "Root $I30 $FILE_NAME copy differs from record {rec_num} ({})",
                        entry.name
                    ),
                ));
            }
        }
    }

    /// Verifies that every active MFT record has its bit set in $MFT::$BITMAP.
    pub(crate) fn check_mft_bitmap_consistency(&mut self, rep: &mut VerifyReport) {
        let Some(mft_rec) = self.read_record_view(MFT_RECORD_MFT) else {
            rep.push(Finding::err(
                "SYS.MFT_BMP",
                "Record 0 ($MFT) is not readable",
            ));
            return;
        };
        let Ok(mft_view) = crate::view::mft_view::MftRecordView::new(&mft_rec) else {
            rep.push(Finding::err("SYS.MFT_BMP", "Record 0 ($MFT) is malformed"));
            return;
        };

        let bmp_attr = match mft_view.find_named(ATTR_BITMAP, None) {
            Ok(Some(a)) => a,
            _ => {
                rep.push(Finding::err(
                    "SYS.MFT_BMP",
                    "$MFT missing $BITMAP attribute",
                ));
                return;
            }
        };

        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        let bmp_bytes = match bmp_attr.is_resident() {
            true => resolver
                .get_resident_attribute_content(bmp_attr)
                .map(|b| b.to_vec()),
            false => resolver.get_non_resident_attribute_content(bmp_attr),
        };

        let Ok(bitmap) = bmp_bytes else {
            rep.push(Finding::err(
                "SYS.MFT_BMP",
                "Failed to read $MFT::$BITMAP stream",
            ));
            return;
        };

        let records_to_check = (bitmap.len() * 8).min(256);
        let mut checked = 0usize;
        let mut mismatches = 0usize;

        for rec_num in 0..records_to_check as u64 {
            let Ok(rec_data) = mft::read_record(self.io, self.meta, rec_num) else {
                continue;
            };
            let Ok(rec_view) = crate::view::mft_view::MftRecordView::new(&rec_data) else {
                continue;
            };
            let is_in_use = (rec_view.header().flags & MftRecordFlags::IN_USE.bits()) != 0;
            let bit_is_set = bitmap.get_bit(rec_num as usize);

            if is_in_use != bit_is_set {
                mismatches += 1;
                rep.push(Finding::err(
                    "SYS.MFT_BMP.MISMATCH",
                    format!("MFT Record {rec_num}: IN_USE is {is_in_use} but $MFT::$BITMAP bit is {bit_is_set}"),
                ));
            }
            checked += 1;
        }

        if mismatches == 0 {
            rep.push(Finding::info(
                "SYS.MFT_BMP",
                format!(
                    "$MFT::$BITMAP consistent with MFT record headers ({checked} records checked)"
                ),
            ));
        }
    }
}

pub(crate) fn standard_info_attrs(view: &crate::view::mft_view::MftRecordView<'_>) -> Option<u32> {
    let attr = view
        .find_named(ATTR_STANDARD_INFORMATION, None)
        .ok()
        .flatten()?;
    let value = attr.as_view().ok()?.as_resident()?;
    let info = *StandardInformation::ref_from_prefix(value).ok()?.0;
    Some(info.file_attributes)
}

pub(crate) fn file_name_attr(
    view: &crate::view::mft_view::MftRecordView<'_>,
) -> Option<FileNameAttribute> {
    let attr = view.find_named(ATTR_FILE_NAME, None).ok().flatten()?;
    let value = attr.as_view().ok()?.as_resident()?;
    Some(*FileNameAttribute::ref_from_prefix(value).ok()?.0)
}

pub(crate) fn resident_index_root_entries(
    view: &crate::view::mft_view::MftRecordView<'_>,
    name: &str,
) -> Option<alloc::vec::Vec<IndexFileNameCopy>> {
    let attr = view
        .find_named(ATTR_INDEX_ROOT, Some(name))
        .ok()
        .flatten()?;
    let value = attr.as_view().ok()?.as_resident()?;
    if value.len()
        < core::mem::size_of::<IndexRootHeader>() + core::mem::size_of::<IndexNodeHeader>()
    {
        return None;
    }

    let node_offset = core::mem::size_of::<IndexRootHeader>();
    let node = *IndexNodeHeader::ref_from_prefix(&value[node_offset..])
        .ok()?
        .0;
    let mut pos = node_offset.checked_add(node.entries_offset as usize)?;
    let end = node_offset.checked_add(node.index_length as usize)?;
    if pos > end || end > value.len() {
        return None;
    }

    let mut entries = alloc::vec::Vec::new();
    while pos + core::mem::size_of::<IndexEntryHeader>() <= end {
        let header = *IndexEntryHeader::ref_from_prefix(&value[pos..]).ok()?.0;
        if header.entry_length == 0 {
            return None;
        }
        if header.flags & crate::flags::IndexEntryFlags::LAST_ENTRY.bits() != 0 {
            break;
        }

        let content_start = pos + core::mem::size_of::<IndexEntryHeader>();
        let content_end = content_start.checked_add(header.content_length as usize)?;
        if content_end > end
            || (header.content_length as usize) < core::mem::size_of::<FileNameAttribute>()
        {
            return None;
        }

        let file_name = *FileNameAttribute::ref_from_prefix(&value[content_start..])
            .ok()?
            .0;
        let name_start = content_start + core::mem::size_of::<FileNameAttribute>();
        let name_end = name_start.checked_add(file_name.filename_length as usize * 2)?;
        if name_end > content_end {
            return None;
        }
        let mut utf16 = alloc::vec::Vec::with_capacity(file_name.filename_length as usize);
        for chunk in value[name_start..name_end].chunks_exact(2) {
            utf16.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        let entry_name = alloc::string::String::from_utf16(&utf16).ok()?;
        entries.push(IndexFileNameCopy {
            name: entry_name,
            mft_reference: header.mft_reference,
            file_name,
        });

        pos = pos.checked_add(header.entry_length as usize)?;
    }

    Some(entries)
}

pub(crate) fn same_file_name_metadata(a: &FileNameAttribute, b: &FileNameAttribute) -> bool {
    a.parent_directory == b.parent_directory
        && a.creation_time == b.creation_time
        && a.modification_time == b.modification_time
        && a.mft_modification_time == b.mft_modification_time
        && a.access_time == b.access_time
        && a.allocated_size == b.allocated_size
        && a.data_size == b.data_size
        && a.file_attributes == b.file_attributes
        && a.packed_ea_size == b.packed_ea_size
        && a.reserved == b.reserved
        && a.filename_length == b.filename_length
        && a.namespace == b.namespace
}
