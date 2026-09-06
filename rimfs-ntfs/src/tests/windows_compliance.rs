// SPDX-License-Identifier: MIT
//! Windows 11 NTFS Compliance Tripwires & Regression Tests
//!
//! These tests strictly lock down the on-disk structures required by
//! Windows 11 CHKDSK and the Windows kernel. Any regression in AttrDef,
//! Bitmap consistency, Quota indexes, UpCase:$Info, or Root DACL will
//! immediately fail here.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::constant::*;
use crate::formatter::NtfsFormatter;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::view::mft_view::MftRecordView;

fn make_formatted_volume(size_mb: u64) -> (NtfsMeta, MemRimIO<'static>) {
    let size_bytes = size_mb * 1024 * 1024;
    let meta = NtfsMeta::new(size_bytes, Some("WIN_TEST")).unwrap();
    let buf = vec![0u8; size_bytes as usize].leak();
    let mut io = MemRimIO::new(buf);
    NtfsFormatter::new(&mut io, &meta).format(true).unwrap();
    (meta, io)
}

#[test]
fn test_attrdef_2560_bytes_and_exact_flags() {
    let (meta, mut io) = make_formatted_volume(32);
    let rec_bytes = mft::read_record(&mut io, &meta, MFT_RECORD_ATTRDEF).unwrap();
    let view = MftRecordView::new(&rec_bytes).unwrap();

    let data_attr = view
        .find(ATTR_DATA)
        .unwrap()
        .expect("$AttrDef must have a $DATA attribute");

    // 1. Data size must be exactly 2560 bytes (16 * 160)
    let (data_size, content_bytes) = match data_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            (value.len() as u64, value.to_vec())
        }
        crate::view::attr_view::AttrView::NonResident {
            data_size, runlist, ..
        } => {
            let mut buf = vec![0u8; data_size as usize];
            let mut off = 0;
            for run in runlist.iter() {
                if let Some(lcn) = run.lcn {
                    let len_bytes = (run.len * meta.bytes_per_cluster as u64) as usize;
                    let chunk = len_bytes.min(buf.len() - off);
                    io.read_at(
                        lcn * meta.bytes_per_cluster as u64,
                        &mut buf[off..off + chunk],
                    )
                    .unwrap();
                    off += chunk;
                }
            }
            (data_size, buf)
        }
    };

    assert_eq!(
        data_size, 2560,
        "$AttrDef data size MUST be exactly 2560 bytes (16 entries * 160)"
    );
    assert_eq!(content_bytes.len(), 2560);

    // 2. 16th entry (bytes 2400..2560) must be all zeros (null terminator required by Windows)
    let terminator = &content_bytes[2400..2560];
    assert!(
        terminator.iter().all(|&b| b == 0),
        "16th entry of $AttrDef MUST be 160 bytes of zeros (null terminator)"
    );

    // 3. Entry 0 ($STANDARD_INFORMATION, 0x10): flags = 0x40, min = 48, max = 72
    let entry0 = &content_bytes[0..160];
    let e0_type = u32::from_le_bytes(entry0[128..132].try_into().unwrap());
    let e0_flags = u32::from_le_bytes(entry0[140..144].try_into().unwrap());
    let e0_min = u64::from_le_bytes(entry0[144..152].try_into().unwrap());
    let e0_max = u64::from_le_bytes(entry0[152..160].try_into().unwrap());
    assert_eq!(e0_type, 0x10);
    assert_eq!(e0_flags, 0x40, "$STANDARD_INFORMATION flags must be 0x40");
    assert_eq!(e0_min, 48);
    assert_eq!(e0_max, 72);

    // 4. Entry 2 ($FILE_NAME, 0x30): flags = 0x42
    let entry2 = &content_bytes[320..480];
    let e2_flags = u32::from_le_bytes(entry2[140..144].try_into().unwrap());
    assert_eq!(
        e2_flags, 0x42,
        "$FILE_NAME flags must be 0x42 (INDEXABLE | RESIDENT)"
    );

    // 5. Entry 4 ($SECURITY_DESCRIPTOR, 0x50): flags = 0x80, max = u64::MAX
    let entry4 = &content_bytes[640..800];
    let e4_flags = u32::from_le_bytes(entry4[140..144].try_into().unwrap());
    let e4_max = u64::from_le_bytes(entry4[152..160].try_into().unwrap());
    assert_eq!(
        e4_flags, 0x80,
        "$SECURITY_DESCRIPTOR flags must be 0x80 (LOG_PRESENCE)"
    );
    assert_eq!(e4_max, u64::MAX);

    // 6. Entry 7 ($DATA, 0x80): flags = 0x00, max = u64::MAX
    let entry7 = &content_bytes[1120..1280];
    let e7_flags = u32::from_le_bytes(entry7[140..144].try_into().unwrap());
    let e7_max = u64::from_le_bytes(entry7[152..160].try_into().unwrap());
    assert_eq!(e7_flags, 0x00, "$DATA flags must be 0x00");
    assert_eq!(e7_max, u64::MAX);
}

#[test]
fn test_bitmap_exact_consistency_and_tail_bits() {
    let (meta, mut io) = make_formatted_volume(32);

    // 1. Read $Bitmap content
    let rec_bytes = mft::read_record(&mut io, &meta, MFT_RECORD_BITMAP).unwrap();
    let view = MftRecordView::new(&rec_bytes).unwrap();
    let data_attr = view.find(ATTR_DATA).unwrap().expect("$Bitmap has $DATA");
    let (bmp_lcn, bmp_len) = match data_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::NonResident { runlist, .. } => {
            let r = runlist.iter().next().unwrap();
            (r.lcn.unwrap(), r.len)
        }
        _ => panic!("$Bitmap must be non-resident"),
    };

    // Bitmap LCN must follow $LogFile directly (no 2-cluster gap)
    let log_clusters = (2 * 1024 * 1024u64)
        .min(meta.total_clusters * meta.bytes_per_cluster as u64 / 10)
        .div_ceil(meta.bytes_per_cluster as u64);
    assert_eq!(
        bmp_lcn,
        meta.logfile_lcn + log_clusters,
        "Bitmap LCN must follow $LogFile with zero gap"
    );

    let mut bmp_data = vec![0u8; (bmp_len * meta.bytes_per_cluster as u64) as usize];
    io.read_at(bmp_lcn * meta.bytes_per_cluster as u64, &mut bmp_data)
        .unwrap();

    // 2. Collect ALL clusters actually allocated to files
    let mut expected_bitmap = vec![0u8; bmp_data.len()];
    let mark_cluster = |bmp: &mut [u8], lcn: u64| {
        let byte_idx = (lcn / 8) as usize;
        let bit_idx = (lcn % 8) as u8;
        bmp[byte_idx] |= 1 << bit_idx;
    };

    // Boot sector cluster 0
    mark_cluster(&mut expected_bitmap, 0);

    // Scan all in-use MFT records 0..1024
    for record_num in 0..1024 {
        let Ok(rec) = mft::read_record(&mut io, &meta, record_num) else {
            continue;
        };
        let Ok(view) = MftRecordView::new(&rec) else {
            continue;
        };
        if !view.header().is_in_use() {
            continue;
        }
        for attr in view.attrs().flatten() {
            if let Ok(crate::view::attr_view::AttrView::NonResident { runlist, .. }) =
                attr.as_view()
            {
                // Skip $BadClus:$Bad (sparse hole)
                if record_num == MFT_RECORD_BADCLUS && attr.name().is_some_and(|n| n == "$Bad") {
                    continue;
                }
                for run in runlist.iter() {
                    if let Some(lcn) = run.lcn {
                        for c in 0..run.len {
                            mark_cluster(&mut expected_bitmap, lcn + c);
                        }
                    }
                }
            }
        }
    }

    // 3. Verify ZERO differences between clusters 0 and total_clusters
    for lcn in 0..meta.total_clusters {
        let byte_idx = (lcn / 8) as usize;
        let bit_idx = (lcn % 8) as u8;
        let actual_bit = (bmp_data[byte_idx] >> bit_idx) & 1;
        let expected_bit = (expected_bitmap[byte_idx] >> bit_idx) & 1;
        assert_eq!(
            actual_bit, expected_bit,
            "Bitmap discrepancy at LCN {lcn}: actual={actual_bit}, expected={expected_bit}"
        );
    }

    // 4. Verify ALL tail bits beyond total_clusters up to bitmap_size_bytes * 8 are set to 1
    let bitmap_total_bits = meta.bitmap_size_bytes * 8;
    for bit in meta.total_clusters..bitmap_total_bits {
        let byte_idx = (bit / 8) as usize;
        let bit_idx = (bit % 8) as u8;
        let bit_val = (bmp_data[byte_idx] >> bit_idx) & 1;
        assert_eq!(
            bit_val, 1,
            "Tail bit {bit} beyond total_clusters ({}) MUST be set to 1 (allocated) to satisfy CHKDSK",
            meta.total_clusters
        );
    }
}

#[test]
fn test_upcase_has_info_stream() {
    let (meta, mut io) = make_formatted_volume(32);
    let rec_bytes = mft::read_record(&mut io, &meta, MFT_RECORD_UPCASE).unwrap();
    let view = MftRecordView::new(&rec_bytes).unwrap();

    // 1. Unnamed $DATA must exist and be 131072 bytes (128 KB)
    let data_attr = view
        .find(ATTR_DATA)
        .unwrap()
        .expect("$UpCase must have unnamed $DATA");
    match data_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::NonResident { data_size, .. } => {
            assert_eq!(data_size, 131072);
        }
        _ => panic!("$UpCase unnamed $DATA must be non-resident"),
    }

    // 2. Named $DATA:$Info must exist, be resident, and have size 32 bytes
    let info_attr = view
        .find_named(ATTR_DATA, Some("$Info"))
        .unwrap()
        .expect("$UpCase MUST have named stream $Info");
    assert!(info_attr.is_resident(), "$UpCase:$Info must be resident");
    match info_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            assert_eq!(value.len(), 32, "$UpCase:$Info must be exactly 32 bytes");
        }
        _ => panic!("$UpCase:$Info must be resident"),
    }
}

#[test]
fn test_quota_objid_reparse_invariants() {
    let (meta, mut io) = make_formatted_volume(32);

    // 1. Inode 24 ($Quota): NO $DATA attribute, has $O (0x11) and $Q (0x10) indexes
    let quota_rec = mft::read_record(&mut io, &meta, MFT_RECORD_QUOTA).unwrap();
    let quota = MftRecordView::new(&quota_rec).unwrap();
    assert!(
        quota.find(ATTR_DATA).unwrap().is_none(),
        "$Quota must NOT have $DATA attribute"
    );
    let o_attr = quota
        .find_named(ATTR_INDEX_ROOT, Some("$O"))
        .unwrap()
        .expect("$Quota has $O index");
    let q_attr = quota
        .find_named(ATTR_INDEX_ROOT, Some("$Q"))
        .unwrap()
        .expect("$Quota has $Q index");

    match o_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            let collation = u32::from_le_bytes(value[4..8].try_into().unwrap());
            assert_eq!(collation, 0x11, "$Quota:$O collation rule must be 0x11");
        }
        _ => panic!("$Quota:$O must be resident"),
    }
    match q_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            let collation = u32::from_le_bytes(value[4..8].try_into().unwrap());
            assert_eq!(collation, 0x10, "$Quota:$Q collation rule must be 0x10");
        }
        _ => panic!("$Quota:$Q must be resident"),
    }

    // 2. Inode 25 ($ObjId): NO $DATA attribute, has $O with collation 0x13
    let objid_rec = mft::read_record(&mut io, &meta, MFT_RECORD_OBJID).unwrap();
    let objid = MftRecordView::new(&objid_rec).unwrap();
    assert!(
        objid.find(ATTR_DATA).unwrap().is_none(),
        "$ObjId must NOT have $DATA attribute"
    );
    let objid_o = objid
        .find_named(ATTR_INDEX_ROOT, Some("$O"))
        .unwrap()
        .expect("$ObjId has $O index");
    match objid_o.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            let collation = u32::from_le_bytes(value[4..8].try_into().unwrap());
            assert_eq!(collation, 0x13, "$ObjId:$O collation rule must be 0x13");
        }
        _ => panic!("$ObjId:$O must be resident"),
    }

    // 3. Inode 26 ($Reparse): NO $DATA attribute, has $R with collation 0x13
    let reparse_rec = mft::read_record(&mut io, &meta, MFT_RECORD_REPARSE).unwrap();
    let reparse = MftRecordView::new(&reparse_rec).unwrap();
    assert!(
        reparse.find(ATTR_DATA).unwrap().is_none(),
        "$Reparse must NOT have $DATA attribute"
    );
    let reparse_r = reparse
        .find_named(ATTR_INDEX_ROOT, Some("$R"))
        .unwrap()
        .expect("$Reparse has $R index");
    match reparse_r.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            let collation = u32::from_le_bytes(value[4..8].try_into().unwrap());
            assert_eq!(collation, 0x13, "$Reparse:$R collation rule must be 0x13");
        }
        _ => panic!("$Reparse:$R must be resident"),
    }
}

#[test]
fn test_root_directory_security_descriptor_228_bytes() {
    let (meta, mut io) = make_formatted_volume(32);
    let root_rec = mft::read_record(&mut io, &meta, MFT_RECORD_ROOT).unwrap();
    let root = MftRecordView::new(&root_rec).unwrap();

    // 1. Must have $SECURITY_DESCRIPTOR (0x50) attribute inline
    let sd_attr = root
        .find(ATTR_SECURITY_DESCRIPTOR)
        .unwrap()
        .expect("Root directory MUST have inline $SECURITY_DESCRIPTOR attribute (0x50)");
    assert!(
        sd_attr.is_resident(),
        "Root $SECURITY_DESCRIPTOR must be resident"
    );

    match sd_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            assert_eq!(
                value.len(),
                228,
                "Root security descriptor MUST be exactly 228 bytes"
            );
            // Check DACL contains Authenticated Users (S-1-5-11) and Users (S-1-5-32-545)
            let has_auth_users = value
                .windows(12)
                .any(|w| w == [1, 1, 0, 0, 0, 0, 0, 5, 11, 0, 0, 0]);
            let has_users = value
                .windows(16)
                .any(|w| w == [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 33, 2, 0, 0]);
            assert!(
                has_auth_users,
                "Root DACL must grant access to Authenticated Users (S-1-5-11)"
            );
            assert!(
                has_users,
                "Root DACL must grant access to Users (S-1-5-32-545)"
            );
        }
        _ => panic!("Root $SECURITY_DESCRIPTOR must be resident"),
    }
}

#[test]
fn test_badclus_sparse_runlist_with_zero_flags() {
    let (meta, mut io) = make_formatted_volume(32);
    let bad_rec = mft::read_record(&mut io, &meta, MFT_RECORD_BADCLUS).unwrap();
    let badclus = MftRecordView::new(&bad_rec).unwrap();
    let bad_attr = badclus
        .find_named(ATTR_DATA, Some("$Bad"))
        .unwrap()
        .expect("$BadClus:$Bad exists");

    // Attribute flags MUST be 0 (NOT 0x8000 SPARSE), length must be 80
    assert_eq!(
        { bad_attr.header.flags },
        0,
        "$BadClus:$Bad flags MUST be 0"
    );
    assert_eq!(
        { bad_attr.header.length },
        80,
        "$BadClus:$Bad attribute length MUST be 80 bytes"
    );

    match bad_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::NonResident { runlist, .. } => {
            let runs: Vec<_> = runlist.iter().collect();
            assert_eq!(runs.len(), 1);
            assert_eq!(runs[0].lcn, None, "Run must be a hole (LCN = None)");
            assert_eq!(runs[0].len, meta.total_clusters);
        }
        _ => panic!("$BadClus:$Bad must be non-resident"),
    }
}

#[test]
fn test_root_directory_preserves_system_entries_and_sd_after_injection() {
    use crate::core::resolver::attr::FileAttributes;
    use crate::injector::NtfsInjector;
    use crate::types::NtfsFileAttributes;
    use rimfs_core::injector::FsTreeInjector;
    use rimfs_core::resolver::FsTreeResolver;

    let (meta, mut io) = make_formatted_volume(32);
    {
        let mut injector = NtfsInjector::new(&mut io, &meta).unwrap();
        let attr = FileAttributes::default();
        injector.set_root_context(&attr).unwrap();

        // Inject files
        let mut dummy_data = [0x42u8; 1024];
        let mut mem_read = MemRimIO::new(&mut dummy_data);
        injector
            .write_file("TEST.TXT", &mut mem_read, 1024, &attr)
            .unwrap();

        injector.write_dir("SUBDIR", &attr).unwrap();
        injector.flush().unwrap();
    }

    // Verify Root MFT record (record 5)
    let root_rec = mft::read_record(&mut io, &meta, MFT_RECORD_ROOT).unwrap();
    let root = MftRecordView::new(&root_rec).unwrap();

    // 1. Security descriptor must STILL be 228 bytes!
    let sd_attr = root
        .find(ATTR_SECURITY_DESCRIPTOR)
        .unwrap()
        .expect("Root directory MUST have inline $SECURITY_DESCRIPTOR attribute (0x50)");
    match sd_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            assert_eq!(
                value.len(),
                228,
                "Root security descriptor MUST remain 228 bytes after injection"
            );
        }
        _ => panic!("Root $SECURITY_DESCRIPTOR must be resident"),
    }

    // 2. Attributes must still be HIDDEN | SYSTEM | DIRECTORY
    let std_attr = root.find(ATTR_STANDARD_INFORMATION).unwrap().unwrap();
    let std_info = match std_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            u32::from_le_bytes([value[32], value[33], value[34], value[35]])
        }
        _ => panic!("Standard Information must be resident"),
    };
    let expected_attrs =
        (NtfsFileAttributes::HIDDEN | NtfsFileAttributes::SYSTEM | NtfsFileAttributes::DIRECTORY)
            .bits();
    assert_eq!(
        std_info, expected_attrs,
        "Root attributes must remain HIDDEN | SYSTEM | DIRECTORY"
    );

    // 3. Root directory must contain all 12 system files + TEST.TXT + SUBDIR
    let mut resolver = crate::resolver::NtfsResolver::new(&mut io, &meta);
    let entries = resolver.read_dir("/").unwrap();
    let expected_names = [
        "$AttrDef", "$BadClus", "$Bitmap", "$Boot", "$Extend", "$LogFile", "$MFT", "$MFTMirr",
        "$Secure", "$UpCase", "$Volume", "TEST.TXT", "SUBDIR",
    ];
    for expected in expected_names {
        assert!(
            entries.iter().any(|e| e == expected),
            "Root directory MUST contain '{}' after injection. Found entries: {:?}",
            expected,
            entries
        );
    }

    // 4. Injected files must have sorted attributes [0x10, 0x30, 0x80] and NO inline 0x50
    let test_file_rec = mft::read_record(&mut io, &meta, 28).unwrap();
    let test_file = MftRecordView::new(&test_file_rec).unwrap();
    let attr_types: Vec<u32> = test_file.attrs().map(|a| a.unwrap().ty()).collect();
    assert_eq!(
        attr_types,
        vec![ATTR_STANDARD_INFORMATION, ATTR_FILE_NAME, ATTR_DATA]
    );
}
