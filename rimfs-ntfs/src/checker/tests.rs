// SPDX-License-Identifier: MIT

//! NTFS consistency checker unit and regression tests.

use super::system::*;
use super::*;
use crate::attr::NtfsFileAttributes;
use crate::formatter::NtfsFormatter;
use crate::types::NtfsAttributeType;
use crate::upcase::UpcaseFlavor;
use crate::{AttrView, flags::*};
#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::vec::Vec;
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
        secure.header().flags.get() & MftRecordFlags::IS_VIEW_INDEX.bits(),
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
        .find_named(NtfsAttributeType::Data, Some("$SDS"))
        .unwrap()
        .unwrap()
        .as_view()
        .unwrap()
    {
        AttrView::NonResident {
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
        .find_named(NtfsAttributeType::Data, Some("$Bad"))
        .unwrap()
        .expect("$BadClus:$Bad exists");
    assert_eq!({ bad_attr.header.flags }, 0);
    match bad_attr.as_view().unwrap() {
        AttrView::NonResident {
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
        .find_named(NtfsAttributeType::Data, None)
        .expect("Valid attribute parse")
        .expect("Unnamed $DATA exists");

    // 3. Verify non-resident and size properties
    assert!(!data_attr.is_resident(), "non-resident == true");
    let attr_view = data_attr.as_view().expect("Valid attr view");
    match attr_view {
        AttrView::NonResident {
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
        .find_named(NtfsAttributeType::FileName, None)
        .expect("Valid attr parse")
        .expect("$FILE_NAME exists");
    let fn_val = resolver.get_resident_attribute_content(fn_attr).unwrap();
    let fn_struct = *crate::types::FileNameAttribute::ref_from_prefix(fn_val)
        .unwrap()
        .0;
    let fn_data_size = fn_struct.data_size.get();
    let fn_allocated_size = fn_struct.allocated_size.get();
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
        .find_named(NtfsAttributeType::IndexRoot, Some("$I30"))
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
        .find_named(NtfsAttributeType::IndexAllocation, Some("$I30"))
        .unwrap()
        .unwrap();
    let alloc_content = resolver
        .get_non_resident_attribute_content(alloc_attr)
        .unwrap();
    assert!(
        alloc_content.len() >= 4096 * 2,
        "Must contain at least 2 INDX blocks"
    );

    for (idx, chunk) in alloc_content.chunks(4096).enumerate() {
        let indx_header = crate::types::IndexRecordHeader::read_from_prefix(chunk)
            .unwrap()
            .0;
        let vcn = indx_header.index_block_vcn.get();
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

    let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);
    let rec = resolver.read_mft_record(MFT_RECORD_ROOT).unwrap();
    let view = crate::view::mft_view::MftRecordView::new(&rec).unwrap();
    let alloc_attr = view
        .find_named(NtfsAttributeType::IndexAllocation, Some("$I30"))
        .unwrap()
        .unwrap();
    let alloc_content = resolver
        .get_non_resident_attribute_content(alloc_attr)
        .unwrap();

    for (idx, chunk) in alloc_content.chunks(4096).enumerate() {
        let indx_header = crate::types::IndexRecordHeader::read_from_prefix(chunk)
            .unwrap()
            .0;
        let vcn = indx_header.index_block_vcn.get();
        assert_eq!(vcn, idx as u64, "Block {idx} header VCN must be idx*1");
    }
}

#[test]
fn test_ntfs_logfile_and_system_records_structure() {
    let meta = NtfsMeta::new(256 * 1024 * 1024, Some("WIN_DATA")).unwrap();
    let mut buffer = vec![0u8; 256 * 1024 * 1024];
    let mut io = MemRimIO::new(&mut buffer);

    NtfsFormatter::new(&mut io, &meta).format(true).unwrap();

    let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);

    let rec0 = resolver.read_mft_record(0).unwrap();
    let view0 = crate::view::mft_view::MftRecordView::new(&rec0).unwrap();
    let seq0 = view0.header().sequence_number.get();
    assert_eq!(seq0, 1);

    let log_rec = resolver.read_mft_record(MFT_RECORD_LOGFILE).unwrap();
    let log_view = crate::view::mft_view::MftRecordView::new(&log_rec).unwrap();
    let log_data_attrs: Vec<_> = log_view
        .attrs()
        .map(|a| a.unwrap())
        .filter(|a| a.ty() == NtfsAttributeType::Data.code())
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
        .find(NtfsAttributeType::Data)
        .unwrap()
        .unwrap();

    let root_rec = resolver.read_mft_record(MFT_RECORD_ROOT).unwrap();
    let root_view = crate::view::mft_view::MftRecordView::new(&root_rec).unwrap();
    let seq5 = root_view.header().sequence_number.get();
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
