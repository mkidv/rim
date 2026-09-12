// SPDX-License-Identifier: MIT

//! exFAT on-disk specification compliance tests.

use crate::types::*;
use zerocopy::IntoBytes;

// Compile-time layout guarantees
const _: () = {
    assert!(core::mem::size_of::<ExFatBootSector>() == 512);
    assert!(core::mem::size_of::<ExFatPrimaryEntry>() == 32);
    assert!(core::mem::size_of::<ExFatStreamEntry>() == 32);
    assert!(core::mem::size_of::<ExFatNameEntry>() == 32);
};

#[test]
fn test_exfat_boot_sector_wire_format_offsets() {
    let meta = crate::meta::ExFatMeta::new(32 * 1024 * 1024, Some("EXFAT_TEST")).unwrap();
    let vbr = ExFatBootSector::new_from_meta(&meta);
    let bytes = vbr.as_bytes();

    assert_eq!(
        &bytes[0..3],
        &[0xEB, 0x76, 0x90],
        "Jump instruction at 0x00"
    );
    assert_eq!(&bytes[3..11], b"EXFAT   ", "FS name at 0x03");
    assert_eq!(
        &bytes[100..104],
        &meta.volume_id.to_le_bytes(),
        "Volume serial at 0x64"
    );
    assert_eq!(&bytes[104..106], &[0x00, 0x01], "FS revision 1.00 at 0x68");
    assert_eq!(
        bytes[108],
        meta.bytes_per_sector.trailing_zeros() as u8,
        "Sector shift at 0x6C"
    );
    assert_eq!(
        bytes[109],
        meta.sectors_per_cluster.trailing_zeros() as u8,
        "Cluster shift at 0x6D"
    );
    assert_eq!(&bytes[510..512], &[0x55, 0xAA], "End signature at 0x1FE");
}

#[test]
fn test_exfat_attribute_canonical_constants() {
    use crate::attr::ExFatAttributes;
    assert_eq!(ExFatAttributes::READ_ONLY.bits(), 0x0001);
    assert_eq!(ExFatAttributes::HIDDEN.bits(), 0x0002);
    assert_eq!(ExFatAttributes::SYSTEM.bits(), 0x0004);
    assert_eq!(ExFatAttributes::DIRECTORY.bits(), 0x0010);
    assert_eq!(ExFatAttributes::ARCHIVE.bits(), 0x0020);
}
