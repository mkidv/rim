// SPDX-License-Identifier: MIT
//! Directory index ($INDEX_ROOT, $INDEX_ALLOCATION, $BITMAP) integrity verification.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::format;
use rimio::RimIO;
use zerocopy::FromBytes;

use crate::constant::*;
use crate::core::bitmap::BitmapOps;
use crate::core::checker::{Finding, FsCheckerResult, VerifyReport};
use crate::meta::NtfsMeta;

/// Validates structural index integrity ($INDEX_ROOT, $INDEX_ALLOCATION, $BITMAP, child VCNs) of a directory.
pub fn check_dir_index_with_resolver<IO: RimIO + ?Sized>(
    meta: &NtfsMeta,
    resolver: &mut crate::resolver::NtfsResolver<IO>,
    mft_num: u64,
    dir_name: &str,
    rep: &mut VerifyReport,
) -> FsCheckerResult<()> {
    let rec = match resolver.read_mft_record(mft_num) {
        Ok(r) => r,
        Err(e) => {
            rep.push(Finding::err(
                "ROOT.REC",
                format!("Failed to read {dir_name} directory record ({mft_num}): {e}"),
            ));
            return Ok(());
        }
    };

    let view = match crate::view::mft_view::MftRecordView::new(&rec) {
        Ok(v) => v,
        Err(e) => {
            rep.push(Finding::err(
                "ROOT.REC",
                format!("Failed to parse {dir_name} MFT record ({mft_num}): {e:?}"),
            ));
            return Ok(());
        }
    };

    // 1. Locate and validate $INDEX_ROOT:$I30
    let root_attr = match view.find_named(ATTR_INDEX_ROOT, Some("$I30")) {
        Ok(Some(a)) => a,
        Ok(None) => match view.find_named(ATTR_INDEX_ROOT, None) {
            Ok(Some(a)) => a,
            _ => {
                rep.push(Finding::err(
                    "IDX.ROOT",
                    format!("{dir_name} (record {mft_num}) missing $INDEX_ROOT attribute"),
                ));
                return Ok(());
            }
        },
        Err(_) => {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!("{dir_name} (record {mft_num}) corrupt $INDEX_ROOT attribute"),
            ));
            return Ok(());
        }
    };

    if !root_attr.is_resident() {
        rep.push(Finding::err(
            "IDX.ROOT",
            format!("{dir_name} $INDEX_ROOT must be resident"),
        ));
        return Ok(());
    }

    let root_content = match resolver.get_resident_attribute_content(root_attr) {
        Ok(c) => c,
        Err(e) => {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!("Failed to read {dir_name} $INDEX_ROOT content: {e}"),
            ));
            return Ok(());
        }
    };

    let min_root_size = core::mem::size_of::<crate::types::IndexRootHeader>()
        + core::mem::size_of::<crate::types::IndexNodeHeader>();
    if root_content.len() < min_root_size {
        rep.push(Finding::err(
            "IDX.ROOT",
            format!(
                "{dir_name} $INDEX_ROOT size {} < minimum required {min_root_size} bytes",
                root_content.len()
            ),
        ));
        return Ok(());
    }

    let root_header = match crate::types::IndexRootHeader::read_from_prefix(root_content) {
        Ok((h, _)) => h,
        Err(_) => {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!("Failed to read IndexRootHeader in {dir_name}"),
            ));
            return Ok(());
        }
    };

    let indexed_attr_type = root_header.indexed_attr_type;
    if indexed_attr_type != ATTR_FILE_NAME {
        rep.push(Finding::err(
            "IDX.ROOT",
            format!(
                "{dir_name} $INDEX_ROOT indexed attribute type 0x{:02X} != 0x30 ($FILE_NAME)",
                indexed_attr_type
            ),
        ));
    }

    let node_header = match crate::types::IndexNodeHeader::read_from_prefix(&root_content[16..]) {
        Ok((h, _)) => h,
        Err(_) => {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!("Failed to read IndexNodeHeader in {dir_name} $INDEX_ROOT"),
            ));
            return Ok(());
        }
    };

    let entries_offset = node_header.entries_offset as usize;
    let index_length = node_header.index_length as usize;
    let start_offset = 16 + entries_offset;
    let end_offset = 16 + index_length;

    if start_offset < 32 || end_offset > root_content.len() || start_offset > end_offset {
        rep.push(Finding::err(
            "IDX.ROOT",
            format!(
                "{dir_name} $INDEX_ROOT invalid node offsets (entries_offset: {entries_offset}, index_length: {index_length}, content_len: {})",
                root_content.len()
            ),
        ));
        return Ok(());
    }

    let mut referenced_child_vcns: alloc::vec::Vec<u64> = alloc::vec::Vec::new();
    let mut curr_offset = start_offset;
    let mut seen_last_entry = false;

    while curr_offset < end_offset {
        if seen_last_entry {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!(
                    "{dir_name} $I30 contains trailing entries/data after LAST_ENTRY at offset {curr_offset}"
                ),
            ));
            break;
        }

        if curr_offset + 16 > root_content.len() {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!("{dir_name} $I30 truncated entry header at offset {curr_offset}"),
            ));
            break;
        }

        let entry_header =
            match crate::types::IndexEntryHeader::read_from_prefix(&root_content[curr_offset..]) {
                Ok((h, _)) => h,
                Err(_) => {
                    rep.push(Finding::err(
                        "IDX.ROOT",
                        format!(
                            "Failed to read IndexEntryHeader at offset {curr_offset} in {dir_name}"
                        ),
                    ));
                    break;
                }
            };

        let elen = entry_header.entry_length as usize;
        if elen < 16 || !elen.is_multiple_of(8) {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!(
                    "{dir_name} $I30 invalid entry length {elen} (must be >= 16 and 8-byte aligned)"
                ),
            ));
            break;
        }

        if curr_offset + elen > root_content.len() {
            rep.push(Finding::err(
                "IDX.ROOT",
                format!("{dir_name} $I30 entry length {elen} exceeds root content bounds"),
            ));
            break;
        }

        let flags = entry_header.flags;
        let has_subnodes = (flags & crate::flags::IndexEntryFlags::HAS_SUBNODES.bits()) != 0;
        let is_last = (flags & crate::flags::IndexEntryFlags::LAST_ENTRY.bits()) != 0;

        if has_subnodes {
            if elen < 24 {
                rep.push(Finding::err(
                    "IDX.ROOT",
                    format!("{dir_name} entry with HAS_SUBNODES too short ({elen} bytes)"),
                ));
            } else {
                let vcn_bytes = &root_content[curr_offset + elen - 8..curr_offset + elen];
                let child_vcn = u64::from_le_bytes(vcn_bytes.try_into().unwrap());
                referenced_child_vcns.push(child_vcn);
            }
        }

        if is_last {
            seen_last_entry = true;
            if curr_offset + elen != end_offset {
                rep.push(Finding::err(
                    "IDX.ROOT",
                    format!(
                        "{dir_name} $I30 LAST_ENTRY ends at {} but node index_length indicates {index_length}",
                        curr_offset + elen - 16
                    ),
                ));
            }
        }

        curr_offset += elen;
    }

    if !seen_last_entry {
        rep.push(Finding::err(
            "IDX.ROOT",
            format!("{dir_name} $I30 index root missing LAST_ENTRY terminator"),
        ));
    } else {
        rep.push(Finding::info(
            "IDX.ROOT",
            format!("{dir_name} $I30 index root valid"),
        ));
    }

    // 2. Validate $INDEX_ALLOCATION & $BITMAP if subnodes are present
    let alloc_attr_opt = view
        .find_named(ATTR_INDEX_ALLOCATION, Some("$I30"))
        .unwrap_or_default();

    if !referenced_child_vcns.is_empty() || (node_header.flags & 1) != 0 {
        let alloc_attr = match alloc_attr_opt {
            Some(a) => a,
            None => {
                rep.push(Finding::err(
                    "IDX.ALLOC",
                    format!("{dir_name} index root references subnodes but missing $INDEX_ALLOCATION attribute"),
                ));
                return Ok(());
            }
        };

        if alloc_attr.is_resident() {
            rep.push(Finding::err(
                "IDX.ALLOC",
                format!("{dir_name} $INDEX_ALLOCATION should be non-resident"),
            ));
        }

        let alloc_content = match resolver.get_non_resident_attribute_content(alloc_attr) {
            Ok(c) => c,
            Err(e) => {
                rep.push(Finding::err(
                    "IDX.ALLOC",
                    format!("Failed to read {dir_name} $INDEX_ALLOCATION stream: {e}"),
                ));
                return Ok(());
            }
        };

        let block_size = meta.index_record_size as usize;
        let num_blocks = alloc_content.len() / block_size;

        // Read $BITMAP:$I30
        let bitmap_bytes = match view.find_named(ATTR_BITMAP, Some("$I30")) {
            Ok(Some(bm_attr)) => {
                if bm_attr.is_resident() {
                    resolver
                        .get_resident_attribute_content(bm_attr)
                        .unwrap_or(&[])
                        .to_vec()
                } else {
                    resolver
                        .get_non_resident_attribute_content(bm_attr)
                        .unwrap_or_default()
                }
            }
            _ => alloc::vec::Vec::new(),
        };

        if bitmap_bytes.is_empty() {
            rep.push(Finding::err(
                "IDX.BITMAP",
                format!("{dir_name} missing or unreadable $BITMAP:$I30 attribute"),
            ));
        }

        let mut allocated_blocks: alloc::vec::Vec<(u64, usize)> = alloc::vec::Vec::new(); // (vcn, block_idx)
        let mut tree_child_vcns: alloc::vec::Vec<u64> = referenced_child_vcns.clone();

        for (idx, chunk) in alloc_content.chunks(block_size).enumerate() {
            if chunk.len() < block_size {
                rep.push(Finding::err(
                    "IDX.ALLOC",
                    format!("{dir_name} $INDEX_ALLOCATION truncated block {idx} (size {} < {block_size})", chunk.len()),
                ));
                continue;
            }

            if &chunk[0..4] != b"INDX" {
                rep.push(Finding::err(
                    "IDX.ALLOC",
                    format!(
                        "{dir_name} block {idx} signature {:?} != b\"INDX\"",
                        &chunk[0..4]
                    ),
                ));
                continue;
            }

            let mut block_buf = chunk.to_vec();
            if !crate::utils::decode_usa_fixup(&mut block_buf, meta.bytes_per_sector as usize) {
                rep.push(Finding::err(
                    "IDX.USA",
                    format!("{dir_name} block {idx} failed USA fixup verification"),
                ));
            }

            let indx_header = match crate::types::IndexRecordHeader::read_from_prefix(&block_buf) {
                Ok((h, _)) => h,
                Err(_) => {
                    rep.push(Finding::err(
                        "IDX.ALLOC",
                        format!("{dir_name} block {idx} failed to read IndexRecordHeader"),
                    ));
                    continue;
                }
            };

            let indx_vcn = indx_header.index_block_vcn;
            let expected_vcn = meta.index_block_to_vcn(idx as u64);
            if indx_vcn != expected_vcn {
                rep.push(Finding::err(
                    "IDX.VCN",
                    format!(
                        "{dir_name} block {idx} header VCN {indx_vcn} mismatch (expected {expected_vcn})"
                    ),
                ));
            }

            allocated_blocks.push((indx_vcn, idx));

            // Check $BITMAP bit
            let is_set = bitmap_bytes.get_bit(idx);
            if !is_set {
                rep.push(Finding::err(
                    "IDX.BITMAP",
                    format!("{dir_name} block {idx} allocated but bit {idx} is 0 in $BITMAP"),
                ));
            }

            // Parse entries inside this INDX block to find any child VCNs
            let indx_node = match crate::types::IndexNodeHeader::read_from_prefix(&block_buf[24..])
            {
                Ok((h, _)) => h,
                Err(_) => continue,
            };

            let b_entries_offset = indx_node.entries_offset as usize;
            let b_index_length = indx_node.index_length as usize;
            let b_start = 24 + b_entries_offset;
            let b_end = 24 + b_index_length;
            if b_start <= b_end && b_end <= block_size {
                let mut b_offset = b_start;
                while b_offset < b_end {
                    if b_offset + 16 > block_size {
                        break;
                    }
                    if let Ok((eh, _)) =
                        crate::types::IndexEntryHeader::read_from_prefix(&block_buf[b_offset..])
                    {
                        let elen = eh.entry_length as usize;
                        if elen < 16 {
                            break;
                        }
                        if (eh.flags & crate::flags::IndexEntryFlags::HAS_SUBNODES.bits()) != 0
                            && elen >= 24
                        {
                            let sub_vcn = u64::from_le_bytes(
                                block_buf[b_offset + elen - 8..b_offset + elen]
                                    .try_into()
                                    .unwrap(),
                            );
                            tree_child_vcns.push(sub_vcn);
                        }
                        if (eh.flags & crate::flags::IndexEntryFlags::LAST_ENTRY.bits()) != 0 {
                            break;
                        }
                        b_offset += elen;
                    } else {
                        break;
                    }
                }
            }
        }

        rep.push(Finding::info(
            "IDX.ALLOC",
            format!("{dir_name} $I30 INDEX_ALLOCATION valid ({num_blocks} blocks)"),
        ));
        rep.push(Finding::info(
            "IDX.BITMAP",
            format!("{dir_name} $I30 bitmap consistent with allocation"),
        ));

        // 3. Cross-reference VCNs
        let mut resolved_vcns = 0usize;
        let available_vcns: alloc::vec::Vec<u64> =
            allocated_blocks.iter().map(|(v, _)| *v).collect();

        for &vcn in &tree_child_vcns {
            if !available_vcns.contains(&vcn) {
                rep.push(Finding::err(
                    "IDX.VCN",
                    format!(
                        "{dir_name} $I30 child references VCN {vcn}, but corresponding INDX record not found (available: {available_vcns:?})"
                    ),
                ));
            } else {
                resolved_vcns += 1;
            }
        }

        rep.push(Finding::info(
            "IDX.VCN",
            format!("{resolved_vcns} child VCN references resolved"),
        ));
        rep.push(Finding::info(
            "IDX.CROSSREF",
            format!("{dir_name} index tree structurally consistent"),
        ));
    } else {
        rep.push(Finding::info(
            "IDX.CROSSREF",
            format!("{dir_name} resident index structurally consistent"),
        ));
    }

    Ok(())
}
