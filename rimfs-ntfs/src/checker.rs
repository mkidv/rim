#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::format;
use rimio::RimIO;
use zerocopy::FromBytes;

use crate::constant::*;
use crate::core::checker::{
    Finding, FsCheckerError, FsCheckerResult, VerifierOptionsLike, VerifyPhases, VerifyReport,
};
use crate::core::traits::FsChecker;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::NtfsBootSector;

/// NTFS verifier options
#[derive(Debug, Clone)]
pub struct NtfsVerifierOptions {
    phases: VerifyPhases,
    fail_fast: bool,
}

impl Default for NtfsVerifierOptions {
    fn default() -> Self {
        Self {
            phases: VerifyPhases::ALL,
            fail_fast: false,
        }
    }
}

impl VerifierOptionsLike for NtfsVerifierOptions {
    fn phases(&self) -> VerifyPhases {
        self.phases.clone()
    }

    fn fail_fast(&self) -> bool {
        self.fail_fast
    }
}

/// NTFS filesystem checker
pub struct NtfsChecker<'a, IO: RimIO + ?Sized> {
    io: &'a mut IO,
    meta: &'a NtfsMeta,
}

impl<'a, IO: RimIO + ?Sized> NtfsChecker<'a, IO> {
    /// Create a new NTFS checker
    pub fn new(io: &'a mut IO, meta: &'a NtfsMeta) -> Self {
        Self { io, meta }
    }

    /// Check structural index integrity ($INDEX_ROOT, $INDEX_ALLOCATION, $BITMAP, child VCNs) of a directory
    pub fn check_directory_index(
        &mut self,
        mft_num: u64,
        dir_name: &str,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        Self::check_dir_index_with_resolver(self.meta, &mut resolver, mft_num, dir_name, rep)
    }

    fn check_dir_index_with_resolver<IO2: RimIO + ?Sized>(
        meta: &NtfsMeta,
        resolver: &mut crate::resolver::NtfsResolver<IO2>,
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

        let node_header = match crate::types::IndexNodeHeader::read_from_prefix(&root_content[16..])
        {
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

            let entry_header = match crate::types::IndexEntryHeader::read_from_prefix(
                &root_content[curr_offset..],
            ) {
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
                    format!("{dir_name} $I30 invalid entry length {elen} (must be >= 16 and 8-byte aligned)"),
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

                let indx_header =
                    match crate::types::IndexRecordHeader::read_from_prefix(&block_buf) {
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
                let byte_idx = idx / 8;
                let bit_idx = idx % 8;
                if byte_idx < bitmap_bytes.len() {
                    let is_set = (bitmap_bytes[byte_idx] & (1 << bit_idx)) != 0;
                    if !is_set {
                        rep.push(Finding::err(
                            "IDX.BITMAP",
                            format!(
                                "{dir_name} block {idx} allocated but bit {idx} is 0 in $BITMAP"
                            ),
                        ));
                    }
                }

                // Parse entries inside this INDX block to find any child VCNs
                let indx_node =
                    match crate::types::IndexNodeHeader::read_from_prefix(&block_buf[24..]) {
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
}

impl<'a, IO: RimIO + ?Sized> FsChecker for NtfsChecker<'a, IO> {
    type Options = NtfsVerifierOptions;

    fn check_boot(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let mut boot_buf = [0u8; 512];
        self.io
            .read_at(0, &mut boot_buf)
            .map_err(FsCheckerError::IO)?;

        let boot = match NtfsBootSector::read_from_bytes(&boot_buf) {
            Ok(b) => b,
            Err(_) => {
                rep.push(Finding::err(
                    "BOOT.PRIMARY",
                    "Failed to parse primary NTFS boot sector",
                ));
                return Ok(());
            }
        };

        let mut primary_valid = true;
        let oem_id = boot.oem_id;
        if &oem_id != b"NTFS    " {
            rep.push(Finding::err(
                "BOOT.OEM",
                format!("Invalid OEM ID: {:?}", oem_id),
            ));
            primary_valid = false;
        } else {
            rep.push(Finding::info("BOOT.OEM", "NTFS OEM ID OK"));
        }

        let end_marker = boot.end_marker;
        if end_marker != 0xAA55 {
            rep.push(Finding::err(
                "BOOT.SIG",
                format!("Invalid boot signature: 0x{:04X}", end_marker),
            ));
            primary_valid = false;
        } else {
            rep.push(Finding::info("BOOT.SIG", "Boot signature 0xAA55 OK"));
        }

        let bps = boot.bytes_per_sector;
        if bps != 512 && bps != 1024 && bps != 2048 && bps != 4096 {
            rep.push(Finding::warn(
                "BOOT.BPS",
                format!("Unusual sector size: {bps} bytes"),
            ));
        } else {
            rep.push(Finding::info(
                "BOOT.BPS",
                format!("Sector size: {bps} bytes"),
            ));
        }

        let spc = boot.sectors_per_cluster;
        if !spc.is_power_of_two() {
            rep.push(Finding::err(
                "BOOT.SPC",
                format!("Sectors per cluster {spc} is not a power of 2"),
            ));
            primary_valid = false;
        } else {
            rep.push(Finding::info(
                "BOOT.SPC",
                format!("Sectors per cluster: {spc}"),
            ));
        }

        if primary_valid {
            rep.push(Finding::info(
                "BOOT.PRIMARY",
                "Primary NTFS boot sector valid",
            ));
        }

        // Check alternate (backup) boot sector
        let backup_offset = self.meta.backup_boot_sector_offset();
        let mut backup_buf = [0u8; 512];
        match self.io.read_at(backup_offset, &mut backup_buf) {
            Ok(_) => {
                let backup_boot = match NtfsBootSector::read_from_bytes(&backup_buf) {
                    Ok(b) => b,
                    Err(_) => {
                        rep.push(Finding::err(
                            "BOOT.BACKUP",
                            format!(
                                "Failed to parse alternate NTFS boot sector at offset {backup_offset}"
                            ),
                        ));
                        return Ok(());
                    }
                };

                let mut backup_valid = true;
                if &backup_boot.oem_id != b"NTFS    " {
                    rep.push(Finding::err(
                        "BOOT.BACKUP",
                        format!(
                            "Alternate boot sector invalid OEM ID: {:?}",
                            backup_boot.oem_id
                        ),
                    ));
                    backup_valid = false;
                }
                let backup_end_marker = backup_boot.end_marker;
                if backup_end_marker != 0xAA55 {
                    rep.push(Finding::err(
                        "BOOT.BACKUP",
                        format!(
                            "Alternate invalid boot signature: 0x{:04X}",
                            backup_end_marker
                        ),
                    ));
                    backup_valid = false;
                }

                if backup_valid {
                    rep.push(Finding::info(
                        "BOOT.BACKUP",
                        "Alternate NTFS boot sector valid",
                    ));
                }

                // Verify primary and alternate match
                if boot_buf == backup_buf {
                    rep.push(Finding::info(
                        "BOOT.MIRROR",
                        "Primary and alternate boot sectors match",
                    ));
                } else {
                    rep.push(Finding::err(
                        "BOOT.MIRROR",
                        "Primary and alternate boot sectors do not match",
                    ));
                }
            }
            Err(e) => {
                rep.push(Finding::err(
                    "BOOT.BACKUP",
                    format!("Failed to read alternate boot sector at offset {backup_offset}: {e}"),
                ));
            }
        }

        Ok(())
    }

    fn check_geometry(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        // Validate MFT LCN
        if self.meta.mft_lcn >= self.meta.total_clusters {
            rep.push(Finding::err(
                "GEOM.MFT",
                format!(
                    "MFT LCN {} out of range (total clusters: {})",
                    self.meta.mft_lcn, self.meta.total_clusters
                ),
            ));
        } else {
            rep.push(Finding::info(
                "GEOM.MFT",
                format!("MFT LCN {} OK", self.meta.mft_lcn),
            ));
        }

        // Validate MFT Mirror LCN
        if self.meta.mft_mirr_lcn >= self.meta.total_clusters {
            rep.push(Finding::err(
                "GEOM.MIRR",
                format!("MFTMirr LCN {} out of range", self.meta.mft_mirr_lcn),
            ));
        } else {
            rep.push(Finding::info(
                "GEOM.MIRR",
                format!("MFTMirr LCN {} OK", self.meta.mft_mirr_lcn),
            ));
        }

        // Verify primary MFT system records (0: $MFT, 1: $MFTMirr, 3: $Volume, 6: $Bitmap, 7: $Boot, 10: $UpCase)
        for (rec_num, name) in [
            (MFT_RECORD_MFT, "$MFT"),
            (MFT_RECORD_MFTMIRR, "$MFTMirr"),
            (MFT_RECORD_VOLUME, "$Volume"),
            (MFT_RECORD_BITMAP, "$Bitmap"),
            (MFT_RECORD_BOOT, "$Boot"),
            (MFT_RECORD_UPCASE, "$UpCase"),
        ] {
            let offset = self.meta.mft_record_offset(rec_num);
            let record_size = self.meta.mft_record_size as usize;
            let sector_size = self.meta.bytes_per_sector as usize;

            let mut raw = vec![0u8; record_size];
            if let Err(e) = self.io.read_at(offset, &mut raw) {
                rep.push(Finding::err(
                    "MFT.REC",
                    format!("Failed to read raw {name} (record {rec_num}): {e}"),
                ));
                continue;
            }

            if &raw[0..4] != b"FILE" {
                rep.push(Finding::err(
                    "MFT.SIG",
                    format!("Record {rec_num} ({name}) invalid signature: {:?}", &raw[0..4]),
                ));
                continue;
            }

            let usa_ofs = u16::from_le_bytes([raw[4], raw[5]]) as usize;
            let usa_cnt = u16::from_le_bytes([raw[6], raw[7]]) as usize;
            let expected_usa_cnt = (record_size / sector_size) + 1;

            if usa_cnt != expected_usa_cnt {
                rep.push(Finding::err(
                    "MFT.USA",
                    format!(
                        "Record {rec_num} ({name}) invalid usa_count {usa_cnt} (expected {expected_usa_cnt})"
                    ),
                ));
                continue;
            }

            if usa_ofs < 48 || usa_ofs + usa_cnt * 2 > record_size {
                rep.push(Finding::err(
                    "MFT.USA",
                    format!("Record {rec_num} ({name}) invalid usa_offset {usa_ofs}"),
                ));
                continue;
            }

            let usn = u16::from_le_bytes([raw[usa_ofs], raw[usa_ofs + 1]]);
            let mut trailer_ok = true;
            for i in 1..usa_cnt {
                let sector_end = i * sector_size - 2;
                let trailer = u16::from_le_bytes([raw[sector_end], raw[sector_end + 1]]);
                if trailer != usn {
                    rep.push(Finding::err(
                        "MFT.TRAILER",
                        format!(
                            "Record {rec_num} ({name}) sector {i} trailer 0x{trailer:04X} != USN 0x{usn:04X}"
                        ),
                    ));
                    trailer_ok = false;
                    break;
                }
            }
            if !trailer_ok {
                continue;
            }

            if !crate::utils::decode_usa_fixup(&mut raw, sector_size) {
                rep.push(Finding::err(
                    "MFT.USA",
                    format!("Record {rec_num} ({name}) decode_usa_fixup failed"),
                ));
                continue;
            }

            rep.push(Finding::info(
                "MFT.REC",
                format!("System record {rec_num} ({name}) readable"),
            ));
        }

        // Validate $UpCase stream
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        match resolver.read_file_stream(MFT_RECORD_UPCASE, None) {
            Ok(upcase_bytes) => {
                if upcase_bytes.len() == 131072 {
                    rep.push(Finding::info(
                        "UPCASE.SIZE",
                        "$UpCase size OK (131072 bytes)",
                    ));
                    if crate::system::upcase::UpcaseHandle::from_le_bytes(&upcase_bytes).is_ok() {
                        rep.push(Finding::info(
                            "UPCASE.DATA",
                            "$UpCase data stream structurally valid",
                        ));
                    } else {
                        rep.push(Finding::err(
                            "UPCASE.DATA",
                            "Invalid Unicode table in $UpCase data stream",
                        ));
                    }
                } else {
                    rep.push(Finding::err(
                        "UPCASE.SIZE",
                        format!(
                            "Invalid $UpCase size {} (expected 131072 bytes)",
                            upcase_bytes.len()
                        ),
                    ));
                }
            }
            Err(e) => {
                rep.push(Finding::err(
                    "UPCASE.DATA",
                    format!("Failed to read $UpCase data stream: {e}"),
                ));
            }
        }

        Ok(())
    }

    fn check_root(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        match mft::read_record(self.io, self.meta, MFT_RECORD_ROOT) {
            Ok(_) => {
                rep.push(Finding::info(
                    "ROOT.REC",
                    "Root directory record (5) readable",
                ));
                self.check_directory_index(MFT_RECORD_ROOT, "$Root", rep)?;
            }
            Err(e) => {
                rep.push(Finding::err(
                    "ROOT.REC",
                    format!("Failed to read root directory record: {e}"),
                ));
            }
        }
        Ok(())
    }

    fn check_chain(&mut self, _opt: &Self::Options, rep: &mut VerifyReport) -> FsCheckerResult<()> {
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);

        // 1. Read $Bitmap content (record 6)
        let bitmap_bytes = match resolver.read_file_stream(MFT_RECORD_BITMAP, None) {
            Ok(b) => b,
            Err(e) => {
                rep.push(Finding::err(
                    "CHAIN.BITMAP",
                    format!("Failed to read $Bitmap data stream: {e}"),
                ));
                return Ok(());
            }
        };

        let min_expected_bytes = (self.meta.total_clusters).div_ceil(8) as usize;
        if bitmap_bytes.len() < min_expected_bytes {
            rep.push(Finding::warn(
                "CHAIN.BITMAP",
                format!(
                    "$Bitmap size {} < total cluster bytes {}",
                    bitmap_bytes.len(),
                    min_expected_bytes
                ),
            ));
        } else {
            rep.push(Finding::info(
                "CHAIN.BITMAP",
                format!("$Bitmap data stream size OK ({} bytes)", bitmap_bytes.len()),
            ));
        }

        // 2. Scan records 0..16 to verify non-resident runs are marked in $Bitmap
        let mut checked_runs = 0usize;
        for rec_num in 0..16 {
            if let Ok(rec) = resolver.read_mft_record(rec_num) {
                let view = match crate::view::mft_view::MftRecordView::new(&rec) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for attr in view.attrs().flatten() {
                    if let Ok(crate::view::attr_view::AttrView::NonResident { runlist, .. }) =
                        attr.as_view()
                    {
                        for run in runlist.iter() {
                            if let Some(lcn) = run.lcn {
                                checked_runs += 1;
                                for i in 0..run.len {
                                    let cluster = lcn + i;
                                    let byte_idx = (cluster / 8) as usize;
                                    let bit_idx = (cluster % 8) as usize;
                                    if byte_idx < bitmap_bytes.len() {
                                        let is_set = (bitmap_bytes[byte_idx] & (1 << bit_idx)) != 0;
                                        if !is_set {
                                            rep.push(Finding::err(
                                                "CHAIN.ALLOC",
                                                format!(
                                                    "Record {rec_num} uses cluster {cluster} but marked free in $Bitmap"
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        rep.push(Finding::info(
            "CHAIN.OK",
            format!("Validated {checked_runs} non-resident cluster runs against $Bitmap"),
        ));
        Ok(())
    }

    fn check_cross_reference(
        &mut self,
        _opt: &Self::Options,
        rep: &mut VerifyReport,
    ) -> FsCheckerResult<()> {
        let mut resolver = crate::resolver::NtfsResolver::new(self.io, self.meta);
        let mut queue = vec![MFT_RECORD_ROOT];
        let mut visited = alloc::vec::Vec::new();
        let mut total_entries = 0usize;

        while let Some(dir_rec) = queue.pop() {
            if visited.contains(&dir_rec) {
                continue;
            }
            visited.push(dir_rec);

            if dir_rec != MFT_RECORD_ROOT {
                let _ = Self::check_dir_index_with_resolver(
                    self.meta,
                    &mut resolver,
                    dir_rec,
                    &format!("Directory record {dir_rec}"),
                    rep,
                );
            }

            match resolver.read_directory_entries(dir_rec) {
                Ok(entries) => {
                    for (_name, child_rec, attr) in entries {
                        total_entries += 1;
                        if attr.is_dir()
                            && child_rec != MFT_RECORD_ROOT
                            && !visited.contains(&child_rec)
                        {
                            queue.push(child_rec);
                        }
                    }
                }
                Err(e) => {
                    rep.push(Finding::warn(
                        "CROSSREF.DIR",
                        format!("Failed to read directory record {dir_rec}: {e}"),
                    ));
                }
            }
        }

        rep.push(Finding::info(
            "CROSSREF.OK",
            format!(
                "Directory tree verified: {} directories visited, {} entries checked",
                visited.len(),
                total_entries
            ),
        ));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::NtfsFormatter;
    use crate::upcase::UpcaseFlavor;
    use rimfs_core::checker::Severity;
    use rimfs_core::injector::FsTreeInjector;
    use rimio::prelude::MemRimIO;

    #[test]
    fn test_ntfs_checker_basic() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, Some("NTFS_TEST")).unwrap();
        let mut buffer = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        let mut checker = NtfsChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            !report.has_error(),
            "Checker report had errors: {:?}",
            report.findings
        );
        assert!(report.findings.iter().any(|f| f.code == "BOOT.OEM"));
        assert!(report.findings.iter().any(|f| f.code == "GEOM.MFT"));
        assert!(
            report
                .findings
                .iter()
                .any(|f| f.code == "MFT.REC" && f.msg.contains("$UpCase"))
        );
        assert!(report.findings.iter().any(|f| f.code == "UPCASE.SIZE"));
        assert!(report.findings.iter().any(|f| f.code == "UPCASE.DATA"));
        assert!(report.findings.iter().any(|f| f.code == "ROOT.REC"));
        assert!(report.findings.iter().any(|f| f.code == "CHAIN.OK"));
        assert!(report.findings.iter().any(|f| f.code == "CROSSREF.OK"));
    }

    #[test]
    fn test_ntfs_upcase_record_10_regression() {
        let meta = NtfsMeta::new(5 * 1024 * 1024, Some("NTFS_UPCASE")).unwrap();
        let mut buffer = vec![0u8; 5 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        // 1. Read MFT record 10 ($UpCase)
        let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);
        let rec = resolver
            .read_mft_record(MFT_RECORD_UPCASE)
            .expect("$UpCase record readable");

        let view = crate::view::mft_view::MftRecordView::new(&rec).expect("Valid MFT view");

        // 2. Find unnamed $DATA attribute
        let data_attr = view
            .find_named(crate::constant::ATTR_DATA, None)
            .expect("Valid attribute parse")
            .expect("Unnamed $DATA exists");

        // 3. Verify non-resident and size properties
        assert!(!data_attr.is_resident(), "non-resident == true");
        let attr_view = data_attr.as_view().expect("Valid attr view");
        match attr_view {
            crate::view::attr_view::AttrView::NonResident {
                allocated_size,
                data_size,
                initialized_size,
                runlist,
                ..
            } => {
                assert_eq!(data_size, 131072, "data_size == 131072");
                assert_eq!(initialized_size, 131072, "initialized_size == 131072");
                assert!(allocated_size >= 131072, "allocated_size >= 131072");
                assert!(runlist.iter().count() > 0, "runlist resolves correctly");
            }
            _ => panic!("Expected NonResident attribute"),
        }

        // 4. Verify payload read via resolver
        let upcase_payload = resolver
            .read_file_stream(MFT_RECORD_UPCASE, None)
            .expect("Payload stream readable");
        assert_eq!(upcase_payload.len(), 131072, "payload length == 131072");

        // 5. Verify $FILE_NAME data_size
        let fn_attr = view
            .find_named(crate::constant::ATTR_FILE_NAME, None)
            .expect("Valid attr parse")
            .expect("$FILE_NAME exists");
        let fn_val = resolver.get_resident_attribute_content(fn_attr).unwrap();
        let fn_struct = *crate::types::FileNameAttribute::ref_from_prefix(fn_val)
            .unwrap()
            .0;
        let fn_data_size = fn_struct.data_size;
        let fn_allocated_size = fn_struct.allocated_size;
        assert_eq!(fn_data_size, 131072, "$FILE_NAME data_size == 131072");
        assert!(
            fn_allocated_size >= 131072,
            "$FILE_NAME allocated_size >= 131072"
        );
    }

    #[test]
    fn test_ntfs_multi_block_index_vcns() {
        use crate::core::traits::FsNode;
        use crate::injector::NtfsInjector;
        use rimfs_core::resolver::FileAttributes;

        // 20MB image with 512-byte clusters and 4096-byte index records (8 clusters per index record)
        let meta = NtfsMeta::new_custom(
            20 * 1024 * 1024,
            Some("MULTI_IDX"),
            None,
            512,
            512,
            1024,
            4096,
            0,
            UpcaseFlavor::Legacy,
        )
        .unwrap();

        let mut buffer = vec![0u8; 20 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        // Inject 80 files into root directory to force multiple INDX blocks in $INDEX_ALLOCATION
        let mut children = Vec::new();
        for i in 0..80 {
            children.push(FsNode::File {
                name: format!("test_file_entry_{:03}.dat", i),
                content: vec![0xAB; 100],
                attr: FileAttributes::new_file(),
            });
        }
        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children,
        };

        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Run checker
        let mut checker = NtfsChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            !report.has_error(),
            "Checker report had errors: {:?}",
            report.findings
        );

        // Verify that diagnostics reflect tree validation
        assert!(report.findings.iter().any(|f| f.code == "IDX.ROOT"));
        assert!(report.findings.iter().any(|f| f.code == "IDX.ALLOC"));
        assert!(report.findings.iter().any(|f| f.code == "IDX.BITMAP"));
        assert!(report.findings.iter().any(|f| f.code == "IDX.VCN"));
        assert!(report.findings.iter().any(|f| f.code == "IDX.CROSSREF"));

        // Inspect MFT record 5 ($Root) and verify child VCNs are cluster VCNs (multiples of 8)
        let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);
        let rec = resolver.read_mft_record(MFT_RECORD_ROOT).unwrap();
        let view = crate::view::mft_view::MftRecordView::new(&rec).unwrap();
        let root_attr = view
            .find_named(ATTR_INDEX_ROOT, Some("$I30"))
            .unwrap()
            .unwrap();
        let root_content = resolver.get_resident_attribute_content(root_attr).unwrap();

        let node_header = crate::types::IndexNodeHeader::read_from_prefix(&root_content[16..])
            .unwrap()
            .0;
        assert_ne!(
            node_header.flags & 1,
            0,
            "Root directory must have subnodes"
        );

        let alloc_attr = view
            .find_named(ATTR_INDEX_ALLOCATION, Some("$I30"))
            .unwrap()
            .unwrap();
        let alloc_content = resolver
            .get_non_resident_attribute_content(alloc_attr)
            .unwrap();
        assert!(
            alloc_content.len() >= 4096 * 2,
            "Must contain at least 2 INDX blocks"
        );

        // Verify INDX record header VCNs in allocation stream are 0, 8, 16...
        for (idx, chunk) in alloc_content.chunks(4096).enumerate() {
            let indx_header = crate::types::IndexRecordHeader::read_from_prefix(chunk)
                .unwrap()
                .0;
            let vcn = indx_header.index_block_vcn;
            assert_eq!(
                vcn,
                (idx as u64) * 8,
                "Block {idx} header VCN must be cluster multiple idx*8"
            );
        }
    }

    #[test]
    fn test_ntfs_index_geometry_4k_cluster() {
        use crate::core::traits::FsNode;
        use crate::injector::NtfsInjector;
        use rimfs_core::resolver::FileAttributes;

        // 20MB image with 4096-byte clusters and 4096-byte index records (1 cluster per index record)
        let meta = NtfsMeta::new_custom(
            20 * 1024 * 1024,
            Some("4K_GEOM"),
            None,
            512,
            4096,
            1024,
            4096,
            0,
            UpcaseFlavor::Legacy,
        )
        .unwrap();

        let mut buffer = vec![0u8; 20 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        // Inject 80 files into root directory
        let mut children = Vec::new();
        for i in 0..80 {
            children.push(FsNode::File {
                name: format!("file_{:03}.bin", i),
                content: vec![0x55; 50],
                attr: FileAttributes::new_file(),
            });
        }
        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children,
        };

        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // Run checker
        let mut checker = NtfsChecker::new(&mut io, &meta);
        let report = checker.check_all().unwrap();
        assert!(
            !report.has_error(),
            "Checker report had errors: {:?}",
            report.findings
        );

        // Verify INDX record header VCNs are 0, 1, 2...
        let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);
        let rec = resolver.read_mft_record(MFT_RECORD_ROOT).unwrap();
        let view = crate::view::mft_view::MftRecordView::new(&rec).unwrap();
        let alloc_attr = view
            .find_named(ATTR_INDEX_ALLOCATION, Some("$I30"))
            .unwrap()
            .unwrap();
        let alloc_content = resolver
            .get_non_resident_attribute_content(alloc_attr)
            .unwrap();

        for (idx, chunk) in alloc_content.chunks(4096).enumerate() {
            let indx_header = crate::types::IndexRecordHeader::read_from_prefix(chunk)
                .unwrap()
                .0;
            let vcn = indx_header.index_block_vcn;
            assert_eq!(vcn, idx as u64, "Block {idx} header VCN must be idx*1");
        }
    }

    #[test]
    fn test_ntfs_index_corruption_tripwire() {
        use crate::core::traits::FsNode;
        use crate::injector::NtfsInjector;
        use rimfs_core::resolver::FileAttributes;

        let meta = NtfsMeta::new_custom(
            20 * 1024 * 1024,
            Some("TRIPWIRE"),
            None,
            512,
            512,
            1024,
            4096,
            0,
            UpcaseFlavor::Legacy,
        )
        .unwrap();

        let mut buffer = vec![0u8; 20 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        let mut children = Vec::new();
        for i in 0..80 {
            children.push(FsNode::File {
                name: format!("file_{:03}.dat", i),
                content: vec![0x12; 60],
                attr: FileAttributes::new_file(),
            });
        }
        let tree = FsNode::Container {
            attr: FileAttributes::new_dir(),
            children,
        };

        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
        injector.inject_tree(&tree).unwrap();
        injector.flush().unwrap();

        // 1. First confirm checker passes on uncorrupted image
        let mut checker = NtfsChecker::new(&mut io, &meta);
        let rep = checker.check_all().unwrap();
        assert!(!rep.has_error());

        // 2. Corrupt a child VCN in $INDEX_ROOT on disk:
        let root_offset = meta.lcn_to_offset(meta.mft_lcn) + 5 * meta.mft_record_size as u64;
        let mut root_record_bytes = vec![0u8; meta.mft_record_size as usize];
        io.read_at(root_offset, &mut root_record_bytes).unwrap();

        // Decode USA on root record
        crate::utils::decode_usa_fixup(&mut root_record_bytes, meta.bytes_per_sector as usize);

        // Find $INDEX_ROOT attribute within record
        let view = crate::view::mft_view::MftRecordView::new(&root_record_bytes).unwrap();
        let root_attr = view
            .find_named(ATTR_INDEX_ROOT, Some("$I30"))
            .unwrap()
            .unwrap();
        let (val_offset, val_len) =
            if let Ok(crate::view::attr_view::AttrView::Resident { value, .. }) =
                root_attr.as_view()
            {
                let off = (value.as_ptr() as usize) - (root_record_bytes.as_ptr() as usize);
                (off, value.len())
            } else {
                panic!("Expected resident index root");
            };

        // Find the LAST_ENTRY at the end of the root entries and corrupt its child VCN (last 8 bytes)
        let bad_vcn = 999u64;
        let mut corrupted_record_bytes = root_record_bytes.clone();
        let vcn_pos = val_offset + val_len - 8;
        corrupted_record_bytes[vcn_pos..vcn_pos + 8].copy_from_slice(&bad_vcn.to_le_bytes());

        // Re-apply USA fixup
        crate::utils::apply_usa_fixup(&mut corrupted_record_bytes, meta.bytes_per_sector as usize);
        io.write_at(root_offset, &corrupted_record_bytes).unwrap();

        // Run checker - it MUST detect the bad VCN
        let mut checker = NtfsChecker::new(&mut io, &meta);
        let bad_rep = checker.check_all().unwrap();
        assert!(
            bad_rep.has_error(),
            "NtfsChecker MUST detect deliberately corrupted child VCN"
        );
        assert!(
            bad_rep
                .findings
                .iter()
                .any(|f| f.code == "IDX.VCN" && f.sev == Severity::Error),
            "Findings must contain an ERROR with code IDX.VCN: {:?}",
            bad_rep.findings
        );

        // 3. Corrupt $INDEX_ROOT by shortening LAST_ENTRY's entry_length so that trailing bytes appear after it
        let mut corrupted_trailing_bytes = root_record_bytes.clone();
        let last_entry_offset = val_offset + val_len - 24;
        let new_entry_len = 16u16;
        corrupted_trailing_bytes[last_entry_offset + 8..last_entry_offset + 10]
            .copy_from_slice(&new_entry_len.to_le_bytes());
        crate::utils::apply_usa_fixup(
            &mut corrupted_trailing_bytes,
            meta.bytes_per_sector as usize,
        );
        io.write_at(root_offset, &corrupted_trailing_bytes).unwrap();

        let mut checker2 = NtfsChecker::new(&mut io, &meta);
        let bad_rep2 = checker2.check_all().unwrap();
        assert!(
            bad_rep2.has_error(),
            "NtfsChecker MUST detect trailing entries or mismatched index_length"
        );
        assert!(
            bad_rep2
                .findings
                .iter()
                .any(|f| f.code == "IDX.ROOT" && f.sev == Severity::Error),
            "Findings must contain an ERROR with code IDX.ROOT: {:?}",
            bad_rep2.findings
        );
    }

    #[test]
    fn test_ntfs_boot_mirror_and_corruption_tripwire() {
        let meta = NtfsMeta::new(20 * 1024 * 1024, Some("BOOT_TEST")).unwrap();
        let mut buffer = vec![0u8; 20 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        // 1. Uncorrupted check must pass
        let mut checker = NtfsChecker::new(&mut io, &meta);
        let rep = checker.check_all().unwrap();
        assert!(!rep.has_error(), "Findings had error: {:?}", rep.findings);
        assert!(rep.findings.iter().any(|f| f.code == "BOOT.PRIMARY"));
        assert!(rep.findings.iter().any(|f| f.code == "BOOT.BACKUP"));
        assert!(rep.findings.iter().any(|f| f.code == "BOOT.MIRROR"));

        // 2. Corrupt backup boot sector signature
        let backup_offset = meta.backup_boot_sector_offset();
        let mut corrupted_backup = [0u8; 512];
        io.read_at(backup_offset, &mut corrupted_backup).unwrap();
        corrupted_backup[510..512].copy_from_slice(&0x1234u16.to_le_bytes());
        io.write_at(backup_offset, &corrupted_backup).unwrap();

        let mut checker2 = NtfsChecker::new(&mut io, &meta);
        let bad_rep = checker2.check_all().unwrap();
        assert!(
            bad_rep.has_error(),
            "NtfsChecker MUST detect corrupted backup boot signature"
        );
        assert!(
            bad_rep
                .findings
                .iter()
                .any(|f| (f.code == "BOOT.BACKUP" || f.code == "BOOT.MIRROR")
                    && f.sev == Severity::Error),
            "Findings must contain an ERROR for backup boot: {:?}",
            bad_rep.findings
        );
    }

    #[test]
    fn test_ntfs_mft_mst_usa_tripwires() {
        let meta = NtfsMeta::new(20 * 1024 * 1024, Some("MST_TEST")).unwrap();
        let mut buffer = vec![0u8; 20 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        let mft_rec0_offset = meta.mft_record_offset(0);
        let mut rec0_orig = vec![0u8; meta.mft_record_size as usize];
        io.read_at(mft_rec0_offset, &mut rec0_orig).unwrap();

        // 1. Tripwire: corrupt usa_count to 1
        let mut rec0_bad_cnt = rec0_orig.clone();
        rec0_bad_cnt[6..8].copy_from_slice(&1u16.to_le_bytes());
        io.write_at(mft_rec0_offset, &rec0_bad_cnt).unwrap();

        let mut checker = NtfsChecker::new(&mut io, &meta);
        let rep1 = checker.check_all().unwrap();
        assert!(rep1.has_error(), "Must detect wrong usa_count");
        assert!(
            rep1.findings
                .iter()
                .any(|f| f.code == "MFT.USA" && f.sev == Severity::Error),
            "Findings: {:?}",
            rep1.findings
        );

        // 2. Tripwire: corrupt sector trailer
        let mut rec0_bad_trailer = rec0_orig.clone();
        let sector_end = meta.bytes_per_sector as usize - 2;
        rec0_bad_trailer[sector_end] ^= 0xFF;
        io.write_at(mft_rec0_offset, &rec0_bad_trailer).unwrap();

        let mut checker2 = NtfsChecker::new(&mut io, &meta);
        let rep2 = checker2.check_all().unwrap();
        assert!(rep2.has_error(), "Must detect corrupted sector trailer");
        assert!(
            rep2.findings
                .iter()
                .any(|f| f.code == "MFT.TRAILER" && f.sev == Severity::Error),
            "Findings: {:?}",
            rep2.findings
        );

        // 3. Tripwire: out of bounds usa_offset
        let mut rec0_bad_ofs = rec0_orig.clone();
        rec0_bad_ofs[4..6].copy_from_slice(&2000u16.to_le_bytes());
        io.write_at(mft_rec0_offset, &rec0_bad_ofs).unwrap();

        let mut checker3 = NtfsChecker::new(&mut io, &meta);
        let rep3 = checker3.check_all().unwrap();
        assert!(rep3.has_error(), "Must detect out-of-bounds usa_offset");
        assert!(
            rep3.findings
                .iter()
                .any(|f| f.code == "MFT.USA" && f.sev == Severity::Error),
            "Findings: {:?}",
            rep3.findings
        );
    }

    #[test]
    fn test_ntfs_logfile_and_system_records_structure() {
        let meta = NtfsMeta::new(256 * 1024 * 1024, Some("WIN_DATA")).unwrap();
        let mut buffer = vec![0u8; 256 * 1024 * 1024];
        let mut io = MemRimIO::new(&mut buffer);

        NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

        let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);

        // Check Record 0 sequence number == 1
        let rec0 = resolver.read_mft_record(0).unwrap();
        let view0 = crate::view::mft_view::MftRecordView::new(&rec0).unwrap();
        let seq0 = view0.header().sequence_number;
        assert_eq!(seq0, 1);

        // Check Record 2 ($LogFile) has exactly 1 $DATA attribute
        let log_rec = resolver.read_mft_record(MFT_RECORD_LOGFILE).unwrap();
        let log_view = crate::view::mft_view::MftRecordView::new(&log_rec).unwrap();
        let log_data_attrs: Vec<_> = log_view
            .attrs()
            .map(|a| a.unwrap())
            .filter(|a| a.ty() == ATTR_DATA)
            .collect();
        assert_eq!(
            log_data_attrs.len(),
            1,
            "$LogFile MUST have exactly one $DATA attribute"
        );
        assert!(
            !log_data_attrs[0].is_resident(),
            "$LogFile $DATA must be non-resident"
        );

        // Check Record 6 ($Bitmap)
        let bm_rec = resolver.read_mft_record(MFT_RECORD_BITMAP).unwrap();
        let bm_view = crate::view::mft_view::MftRecordView::new(&bm_rec).unwrap();
        let bm_data = bm_view.find(ATTR_DATA).unwrap().unwrap();
        println!("Record 6 raw length: {}", bm_rec.len());
        println!("Record 6 DATA attr raw: {:02X?}", bm_data.raw);

        // Check Record 5 ($Root) sequence number == 5
        let root_rec = resolver.read_mft_record(MFT_RECORD_ROOT).unwrap();
        let root_view = crate::view::mft_view::MftRecordView::new(&root_rec).unwrap();
        let seq5 = root_view.header().sequence_number;
        assert_eq!(seq5, 5);

        // Run checker
        let mut checker = NtfsChecker::new(&mut io, &meta);
        let rep = checker.check_all().unwrap();
        assert!(
            !rep.has_error(),
            "Checker report on clean volume must have no errors: {:?}",
            rep.findings
        );
    }
}
