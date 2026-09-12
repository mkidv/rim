// SPDX-License-Identifier: MIT

//! Tests validating exact Microsoft NTFS v3.1 on-disk specification and Windows compliance.

#[cfg(all(not(feature = "std"), feature = "alloc"))]
use alloc::{vec, vec::Vec};

use rimio::prelude::*;

use crate::attr::NtfsFileAttributes;
use crate::constant::*;
use crate::formatter::NtfsFormatter;
use crate::meta::NtfsMeta;
use crate::mft;
use crate::types::*;
use crate::view::mft_view::MftRecordView;

// Compile-time layout guarantees: verified at `cargo check` with zero runtime overhead.
const _: () = {
    assert!(core::mem::size_of::<MftRecordHeader>() == 48);
    assert!(core::mem::size_of::<AttributeHeader>() == 16);
    assert!(core::mem::size_of::<ResidentAttributeHeader>() == 8);
    assert!(core::mem::size_of::<NonResidentAttributeHeader>() == 48);
    assert!(core::mem::size_of::<IndexEntryHeader>() == 16);
    assert!(core::mem::size_of::<IndexRecordHeader>() == 24);
    assert!(core::mem::size_of::<NtfsBootSector>() == 512);
    assert!(core::mem::size_of::<FileNameAttribute>() == 66);
    assert!(core::mem::size_of::<FullResidentAttributeHeader>() == 24);
    assert!(core::mem::size_of::<FullNonResidentAttributeHeader>() == 64);
};

// =========================================================================
// Canonical Golden Byte Fixtures (Windows NTFS Security Descriptors)
// =========================================================================

/// Golden bytes for SECURITY_DESCRIPTOR_EVERYONE (164 bytes).
pub const GOLDEN_SECURITY_EVERYONE: [u8; 164] = [
    0x01, 0x00, 0x04, 0x80, 0x88, 0x00, 0x00, 0x00, 0x94, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x14, 0x00, 0x00, 0x00, 0x02, 0x00, 0x74, 0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x03, 0x14, 0x00,
    0xff, 0x01, 0x1f, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x03, 0x14, 0x00, 0xff, 0x01, 0x1f, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x0b, 0x00, 0x00, 0x00, 0x00, 0x03, 0x14, 0x00, 0xff, 0x01, 0x1f, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00, 0x00, 0x03, 0x18, 0x00, 0xff, 0x01, 0x1f, 0x00,
    0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x20, 0x02, 0x00, 0x00,
    0x00, 0x03, 0x18, 0x00, 0xff, 0x01, 0x1f, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x20, 0x00, 0x00, 0x00, 0x21, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x12, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00,
    0x20, 0x02, 0x00, 0x00,
];

/// Golden bytes for SECURITY_DESCRIPTOR_SYSTEM (100 bytes).
pub const GOLDEN_SECURITY_SYSTEM: [u8; 100] = [
    0x01, 0x00, 0x04, 0x80, 0x48, 0x00, 0x00, 0x00, 0x54, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x14, 0x00, 0x00, 0x00, 0x02, 0x00, 0x34, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00,
    0x9f, 0x01, 0x12, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x18, 0x00, 0x9f, 0x01, 0x12, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x20, 0x00, 0x00, 0x00, 0x20, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x12, 0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00,
    0x20, 0x02, 0x00, 0x00,
];

/// Golden bytes for SECURITY_DESCRIPTOR_ROOT (228 bytes).
pub const GOLDEN_SECURITY_ROOT: [u8; 228] = [
    0x01, 0x00, 0x04, 0x80, 0xcc, 0x00, 0x00, 0x00, 0xd8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x14, 0x00, 0x00, 0x00, 0x02, 0x00, 0xb8, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00,
    0xff, 0x01, 0x1f, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00,
    0x20, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x18, 0x00, 0x00, 0x00, 0x00, 0x10, 0x01, 0x02, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x20, 0x02, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00,
    0xff, 0x01, 0x1f, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00,
    0x00, 0x0b, 0x14, 0x00, 0x00, 0x00, 0x00, 0x10, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x14, 0x00, 0xbf, 0x01, 0x13, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x0b, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x14, 0x00, 0x00, 0x00, 0x01, 0xe0,
    0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x0b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x00,
    0xa9, 0x00, 0x12, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00,
    0x21, 0x02, 0x00, 0x00, 0x00, 0x0b, 0x18, 0x00, 0x00, 0x00, 0x00, 0xa0, 0x01, 0x02, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x20, 0x00, 0x00, 0x00, 0x21, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x05, 0x12, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05,
    0x12, 0x00, 0x00, 0x00,
];

#[test]
fn test_security_descriptors_match_canonical_golden_bytes() {
    for (descriptor, expected) in [
        (SECURITY_DESCRIPTOR_EVERYONE, &GOLDEN_SECURITY_EVERYONE[..]),
        (SECURITY_DESCRIPTOR_SYSTEM, &GOLDEN_SECURITY_SYSTEM[..]),
        (SECURITY_DESCRIPTOR_ROOT, &GOLDEN_SECURITY_ROOT[..]),
    ] {
        assert_eq!(descriptor.try_to_bytes().unwrap().as_slice(), expected);
        let mut prefixed = Vec::from([0xAA; 3]);
        descriptor.encode_into(&mut prefixed).unwrap();
        assert_eq!(&prefixed[..3], &[0xAA; 3]);
        assert_eq!(&prefixed[3..], expected);
    }
    assert_eq!(FileAccessMask::FILE_READ_EXEC.bits(), 0x001200A9);
}

// =========================================================================
// Wire Format & Offset Checks
// =========================================================================

#[test]
fn test_ntfs_boot_sector_wire_format_offsets() {
    use zerocopy::IntoBytes;
    let meta = crate::meta::NtfsMeta::new(5 * 1024 * 1024, Some("TEST")).unwrap();
    let boot = NtfsBootSector::new_from_meta(&meta);

    let bytes = boot.as_bytes();
    assert_eq!(
        &bytes[0..3],
        &[0xEB, 0x52, 0x90],
        "Jump instruction at 0x00"
    );
    assert_eq!(&bytes[3..11], b"NTFS    ", "OEM ID at 0x03");
    assert_eq!(
        &bytes[11..13],
        &512u16.to_le_bytes(),
        "Bytes per sector at 0x0B"
    );
    assert_eq!(
        bytes[13], meta.sectors_per_cluster,
        "Sectors per cluster at 0x0D"
    );
    assert_eq!(bytes[21], 0xF8, "Media descriptor at 0x15");
    assert_eq!(
        &bytes[48..56],
        &meta.mft_lcn.to_le_bytes(),
        "MFT LCN at 0x30"
    );
    assert_eq!(
        &bytes[56..64],
        &meta.mft_mirr_lcn.to_le_bytes(),
        "MFT Mirr LCN at 0x38"
    );
    assert_eq!(
        &bytes[510..512],
        &[0x55, 0xAA],
        "End marker signature at 0x1FE"
    );
}

#[test]
fn test_ntfs_canonical_constants_and_flags() {
    // Attribute types
    assert_eq!(NtfsAttributeType::StandardInformation.code(), 0x10);
    assert_eq!(NtfsAttributeType::AttributeList.code(), 0x20);
    assert_eq!(NtfsAttributeType::FileName.code(), 0x30);
    assert_eq!(NtfsAttributeType::ObjectId.code(), 0x40);
    assert_eq!(NtfsAttributeType::SecurityDescriptor.code(), 0x50);
    assert_eq!(NtfsAttributeType::VolumeName.code(), 0x60);
    assert_eq!(NtfsAttributeType::VolumeInformation.code(), 0x70);
    assert_eq!(NtfsAttributeType::Data.code(), 0x80);
    assert_eq!(NtfsAttributeType::IndexRoot.code(), 0x90);
    assert_eq!(NtfsAttributeType::IndexAllocation.code(), 0xA0);
    assert_eq!(NtfsAttributeType::Bitmap.code(), 0xB0);
    assert_eq!(NtfsAttributeType::ReparsePoint.code(), 0xC0);
    assert_eq!(NtfsAttributeType::EaInformation.code(), 0xD0);
    assert_eq!(NtfsAttributeType::Ea.code(), 0xE0);
    assert_eq!(NtfsAttributeType::LoggedUtilityStream.code(), 0x100);
    assert_eq!(NtfsAttributeType::End.code(), 0xFFFFFFFF);

    // MFT entry flags
    assert_eq!(MftRecordFlags::IN_USE.bits(), 0x0001);
    assert_eq!(MftRecordFlags::IS_DIRECTORY.bits(), 0x0002);
    assert_eq!(MftRecordFlags::IN_EXTEND.bits(), 0x0004);
    assert_eq!(MftRecordFlags::IS_VIEW_INDEX.bits(), 0x0008);

    // Attribute flags
    assert_eq!(AttributeFlags::COMPRESSED.bits(), 0x0001);
    assert_eq!(AttributeFlags::ENCRYPTED.bits(), 0x4000);
    assert_eq!(AttributeFlags::SPARSE.bits(), 0x8000);
    assert_eq!(NtfsFileAttributes::I30_INDEX.bits(), 0x1000_0000);
    assert_eq!(NtfsFileAttributes::VIEW_INDEX.bits(), 0x2000_0000);

    // Index flags
    assert_eq!(IndexEntryFlags::HAS_SUBNODES.bits(), 0x01);
    assert_eq!(IndexEntryFlags::LAST_ENTRY.bits(), 0x02);
    assert_eq!(IndexNodeFlags::HAS_CHILDREN.bits(), 0x01);
}

#[test]
fn test_non_resident_header_serialization() {
    use zerocopy::IntoBytes;
    let header = NonResidentAttributeHeader {
        lowest_vcn: (0x1122334455667788).into(),
        highest_vcn: (0x99AABBCCDDEEFF00).into(),
        data_runs_offset: (0x1234).into(),
        compression_unit: (0x5678).into(),
        padding: (0).into(),
        allocated_size: (0xAAAA_BBBB_CCCC_DDDD).into(),
        data_size: (0x1111_2222_3333_4444).into(),
        initialized_size: (0x5555_6666_7777_8888).into(),
    };

    let bytes = header.as_bytes();

    assert_eq!(&bytes[0..8], &0x1122334455667788u64.to_le_bytes());
    assert_eq!(&bytes[8..16], &0x99AABBCCDDEEFF00u64.to_le_bytes());
    assert_eq!(&bytes[16..18], &0x1234u16.to_le_bytes());
    assert_eq!(&bytes[18..20], &0x5678u16.to_le_bytes());
    assert_eq!(&bytes[20..24], &[0, 0, 0, 0]);
    assert_eq!(&bytes[24..32], &0xAAAA_BBBB_CCCC_DDDDu64.to_le_bytes());
    assert_eq!(&bytes[32..40], &0x1111_2222_3333_4444u64.to_le_bytes());
    assert_eq!(&bytes[40..48], &0x5555_6666_7777_8888u64.to_le_bytes());
}

// =========================================================================
// NTFS v3.1 / Windows 11 On-Disk Invariant Checks
// =========================================================================

fn make_formatted_volume(size_mb: u64) -> (NtfsMeta, MemRimIO<'static>) {
    let size_bytes = size_mb * 1024 * 1024;
    let meta = NtfsMeta::new(size_bytes, Some("SPEC_TEST")).unwrap();
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
        .find(NtfsAttributeType::Data)
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
    let data_attr = view
        .find(NtfsAttributeType::Data)
        .unwrap()
        .expect("$Bitmap has $DATA");
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
}

#[test]
fn test_upcase_has_info_stream() {
    let (meta, mut io) = make_formatted_volume(32);
    let rec_bytes = mft::read_record(&mut io, &meta, MFT_RECORD_UPCASE).unwrap();
    let view = MftRecordView::new(&rec_bytes).unwrap();

    // 1. Unnamed $DATA must exist and be 131072 bytes (128 KB)
    let data_attr = view
        .find(NtfsAttributeType::Data)
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
        .find_named(NtfsAttributeType::Data, Some("$Info"))
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
        quota.find(NtfsAttributeType::Data).unwrap().is_none(),
        "$Quota must NOT have $DATA attribute"
    );
    let o_attr = quota
        .find_named(NtfsAttributeType::IndexRoot, Some("$O"))
        .unwrap()
        .expect("$Quota has $O index");
    let q_attr = quota
        .find_named(NtfsAttributeType::IndexRoot, Some("$Q"))
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
        objid.find(NtfsAttributeType::Data).unwrap().is_none(),
        "$ObjId must NOT have $DATA attribute"
    );
    let objid_o = objid
        .find_named(NtfsAttributeType::IndexRoot, Some("$O"))
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
        reparse.find(NtfsAttributeType::Data).unwrap().is_none(),
        "$Reparse must NOT have $DATA attribute"
    );
    let reparse_r = reparse
        .find_named(NtfsAttributeType::IndexRoot, Some("$R"))
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
        .find(NtfsAttributeType::SecurityDescriptor)
        .unwrap()
        .expect("Root directory MUST have inline $SECURITY_DESCRIPTOR attribute (0x50)");

    // 2. Length must be exactly 228 bytes
    match sd_attr.as_view().unwrap() {
        crate::view::attr_view::AttrView::Resident { value, .. } => {
            assert_eq!(
                value.len(),
                228,
                "Root $SECURITY_DESCRIPTOR must be exactly 228 bytes"
            );

            assert_eq!(
                value,
                &GOLDEN_SECURITY_ROOT[..],
                "Root $SECURITY_DESCRIPTOR must exactly match canonical golden root descriptor"
            );

            // Verify specific SIDs are present in DACL
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
        .find_named(NtfsAttributeType::Data, Some("$Bad"))
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

    let root_rec = mft::read_record(&mut io, &meta, MFT_RECORD_ROOT).unwrap();
    let root = MftRecordView::new(&root_rec).unwrap();

    // 1. Security descriptor must STILL be 228 bytes!
    let sd_attr = root
        .find(NtfsAttributeType::SecurityDescriptor)
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
    let std_attr = root
        .find(NtfsAttributeType::StandardInformation)
        .unwrap()
        .unwrap();
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
        vec![
            NtfsAttributeType::StandardInformation.code(),
            NtfsAttributeType::FileName.code(),
            NtfsAttributeType::Data.code()
        ]
    );
}
