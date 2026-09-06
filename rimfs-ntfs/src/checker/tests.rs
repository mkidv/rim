use super::system::*;
use super::*;
use crate::flags::*;
use crate::formatter::NtfsFormatter;
use crate::upcase::UpcaseFlavor;
use rimfs_core::checker::Severity;
use rimfs_core::injector::FsTreeInjector;
use rimfs_core::testing::assert_no_errors;
use rimio::prelude::*;

#[test]
fn test_ntfs_checker_basic() {
    let meta = NtfsMeta::new(5 * 1024 * 1024, Some("NTFS_TEST")).unwrap();
    let mut buffer = vec![0u8; 5 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut buffer);

    NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_errors(&report);
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
    assert!(report.findings.iter().any(|f| f.code == "SYS.SECURE"));
    assert!(report.findings.iter().any(|f| f.code == "SYS.BADCLUS"));
    assert!(report.findings.iter().any(|f| f.code == "SYS.EXTEND"));
}

#[test]
fn test_ntfs_windows_system_file_invariants() {
    let meta = NtfsMeta::new(20 * 1024 * 1024, Some("WIN_SYS")).unwrap();
    let mut buffer = vec![0u8; 20 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut buffer);

    NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_errors(&report);

    let secure_rec = mft::read_record(&mut io, &meta, MFT_RECORD_SECURE).unwrap();
    let secure = crate::view::mft_view::MftRecordView::new(&secure_rec).unwrap();
    assert_ne!(
        secure.header().flags & MftRecordFlags::IS_VIEW_INDEX.bits(),
        0,
        "$Secure must carry MFT IS_VIEW_INDEX"
    );
    assert_eq!(
        standard_info_attrs(&secure).unwrap() & NtfsFileAttributes::VIEW_INDEX.bits(),
        NtfsFileAttributes::VIEW_INDEX.bits()
    );
    assert_eq!(
        file_name_attr(&secure).unwrap().file_attributes & NtfsFileAttributes::VIEW_INDEX.bits(),
        NtfsFileAttributes::VIEW_INDEX.bits()
    );
    match secure
        .find_named(ATTR_DATA, Some("$SDS"))
        .unwrap()
        .unwrap()
        .as_view()
        .unwrap()
    {
        crate::view::attr_view::AttrView::NonResident {
            allocated_size,
            data_size,
            initialized_size,
            ..
        } => {
            assert!(data_size > 0);
            assert!(allocated_size >= data_size);
            assert_eq!(initialized_size, data_size);
            assert_ne!(
                allocated_size, data_size,
                "$SDS logical size must not be cluster-rounded"
            );
        }
        _ => panic!("$Secure:$SDS must be non-resident"),
    }

    let bad_rec = mft::read_record(&mut io, &meta, MFT_RECORD_BADCLUS).unwrap();
    let badclus = crate::view::mft_view::MftRecordView::new(&bad_rec).unwrap();
    let bad_attr = badclus
        .find_named(ATTR_DATA, Some("$Bad"))
        .unwrap()
        .expect("$BadClus:$Bad exists");
    assert_eq!({ bad_attr.header.flags }, 0);
    match bad_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::NonResident {
            data_size,
            initialized_size,
            highest_vcn,
            runlist,
            ..
        } => {
            assert_eq!(
                data_size,
                meta.total_clusters * meta.bytes_per_cluster as u64
            );
            assert_eq!(initialized_size, 0);
            assert_eq!(highest_vcn, meta.total_clusters - 1);
            let runs: Vec<_> = runlist.iter().collect();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].lcn, None);
            assert_eq!(runs[0].len, meta.total_clusters);
        }
        _ => panic!("$BadClus:$Bad must be non-resident"),
    }

    let extend_rec = mft::read_record(&mut io, &meta, MFT_RECORD_EXTEND).unwrap();
    let extend = crate::view::mft_view::MftRecordView::new(&extend_rec).unwrap();
    let entries = resident_index_root_entries(&extend, "$I30").unwrap();
    assert_eq!(entries.len(), 3);

    for record in [MFT_RECORD_OBJID, MFT_RECORD_QUOTA, MFT_RECORD_REPARSE] {
        let rec = mft::read_record(&mut io, &meta, record).unwrap();
        let view = crate::view::mft_view::MftRecordView::new(&rec).unwrap();
        assert!(
            view.header().is_in_use(),
            "$Extend child record {record} must be in use"
        );
    }

    for record in MFT_RECORD_RESERVED_START..MFT_RECORD_FREE_START {
        let rec = mft::read_record(&mut io, &meta, record).unwrap();
        let view = crate::view::mft_view::MftRecordView::new(&rec).unwrap();
        assert!(
            view.header().is_in_use(),
            "reserved placeholder record {record} must be in use"
        );
    }
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
        children.push(FsNode::new_file(
            format!("test_file_entry_{:03}.dat", i),
            vec![0xAB; 100],
        ));
    }
    let mut tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children,
    };

    let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_errors(&report);

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
        children.push(FsNode::new_file(
            format!("file_{:03}.bin", i),
            vec![0x55; 50],
        ));
    }
    let mut tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children,
    };

    let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let report = checker.check_all().unwrap();
    assert_no_errors(&report);

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
        children.push(FsNode::new_file(
            format!("file_{:03}.dat", i),
            vec![0x12; 60],
        ));
    }
    let mut tree = FsNode::Container {
        attr: FileAttributes::new_dir(),
        children,
    };

    let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
    injector.inject_tree(&mut tree).unwrap();
    injector.flush().unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let rep = checker.check_all().unwrap();
    assert_no_errors(&rep);

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
        if let Ok(crate::view::attr_view::AttrView::Resident { value, .. }) = root_attr.as_view() {
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

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let rep = checker.check_all().unwrap();
    assert_no_errors(&rep);
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

    let bm_rec = resolver.read_mft_record(MFT_RECORD_BITMAP).unwrap();
    crate::view::mft_view::MftRecordView::new(&bm_rec)
        .unwrap()
        .find(ATTR_DATA)
        .unwrap()
        .unwrap();

    let root_rec = resolver.read_mft_record(MFT_RECORD_ROOT).unwrap();
    let root_view = crate::view::mft_view::MftRecordView::new(&root_rec).unwrap();
    let seq5 = root_view.header().sequence_number;
    assert_eq!(seq5, 5);

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let rep = checker.check_all().unwrap();
    assert_no_errors(&rep);
}

#[test]
fn test_ntfs_mft_bitmap_consistency() {
    let meta = NtfsMeta::new(10 * 1024 * 1024, Some("NTFS_BMP")).unwrap();
    let mut buffer = vec![0u8; 10 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut buffer);

    NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

    let mut checker = NtfsChecker::new(&mut io, &meta);
    let mut rep = VerifyReport::default();
    checker.check_mft_bitmap_consistency(&mut rep);
    assert_no_errors(&rep);
    assert!(rep.findings.iter().any(|f| f.code == "SYS.MFT_BMP"));
}
