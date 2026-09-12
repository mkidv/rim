// SPDX-License-Identifier: MIT

//! ISO 9660 on-disk specification compliance tests (offsets, magic, canonical values).

use crate::types::*;

#[test]
fn test_iso_canonical_constants() {
    assert_eq!(ISO_SECTOR_SIZE, 2048);
    assert_eq!(ISO_STANDARD_ID, b"CD001");
    assert_eq!(VD_PRIMARY, 1);
    assert_eq!(VD_SUPPLEMENTARY, 2);
    assert_eq!(VD_TERMINATOR, 255);
    assert_eq!(DIR_FLAG_DIRECTORY, 0x02);
}
